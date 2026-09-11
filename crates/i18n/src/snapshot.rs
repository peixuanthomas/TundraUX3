use crate::{
    DEFAULT_LANGUAGE, EMBEDDED_FILES, EMBEDDED_MANIFEST, LanguageError, LanguageErrorKind,
    LocalizedMessage, LocalizedText, MessageArg, RepairDiagnostic, RepairKind,
    canonical_language_code,
};
use crate::{
    catalog::parse_manifest,
    resource::{self, Source},
};
use fluent_bundle::{FluentArgs, FluentResource, concurrent::FluentBundle};
use std::{
    collections::BTreeSet,
    fmt,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

type Bundle = FluentBundle<FluentResource>;

/// All resources are owned. Rendering never reads files, consults environment, or mutates locale state.
pub struct LanguageSnapshot {
    code: String,
    generation: u64,
    current: Bundle,
    english: Bundle,
    embedded: Bundle,
    emergency: Bundle,
    current_sources: Vec<Source>,
    english_sources: Vec<Source>,
    embedded_sources: Vec<Source>,
}

impl fmt::Debug for LanguageSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LanguageSnapshot")
            .field("code", &self.code)
            .field("generation", &self.generation)
            .field("current_sources", &self.current_sources)
            .field("english_sources", &self.english_sources)
            .finish_non_exhaustive()
    }
}
impl PartialEq for LanguageSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.code == other.code
            && self.generation == other.generation
            && self.current_sources == other.current_sources
            && self.english_sources == other.english_sources
            && self.embedded_sources == other.embedded_sources
    }
}
impl Eq for LanguageSnapshot {}

#[derive(Debug)]
pub struct LanguageLoad {
    pub snapshot: LanguageSnapshot,
    pub diagnostics: Vec<RepairDiagnostic>,
}

impl LanguageSnapshot {
    pub fn code(&self) -> &str {
        &self.code
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Repair English defaults, then validate the entire requested locale before returning a candidate.
    pub fn load(
        root: impl AsRef<Path>,
        code: &str,
        generation: u64,
    ) -> Result<LanguageLoad, LanguageError> {
        let mut diagnostics = Vec::new();
        let snapshot =
            match Self::load_inner(root.as_ref(), code, generation, false, &mut diagnostics) {
                Ok(snapshot) => snapshot,
                Err(mut error) => {
                    // Repairing English is an independent side effect, even when the
                    // requested language cannot become a publishable candidate.
                    diagnostics.append(&mut error.diagnostics);
                    error.diagnostics = diagnostics;
                    return Err(error);
                }
            };
        Ok(LanguageLoad {
            snapshot,
            diagnostics,
        })
    }

    /// Startup keeps valid translation files and always returns an in-memory English fallback.
    pub fn load_startup(root: impl AsRef<Path>, code: &str, generation: u64) -> LanguageLoad {
        let root = root.as_ref();
        // The startup operation owns diagnostics so a failed locale candidate cannot
        // discard repairs already persisted while preparing its English fallback.
        let mut diagnostics = Vec::new();
        let snapshot = match Self::load_inner(root, code, generation, true, &mut diagnostics) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let mut fallback_diagnostics = Vec::new();
                let snapshot = Self::load_inner(
                    root,
                    DEFAULT_LANGUAGE,
                    generation,
                    true,
                    &mut fallback_diagnostics,
                )
                .unwrap_or_else(|_| Self::embedded(generation));
                // A read-only root can produce the same failure on both attempts.
                // Preserve distinct outcomes while reporting identical issues once.
                for diagnostic in fallback_diagnostics {
                    if !diagnostics.contains(&diagnostic) {
                        diagnostics.push(diagnostic);
                    }
                }
                diagnostics.push(RepairDiagnostic {
                    kind: RepairKind::StartupFallback,
                    path: error.path.clone().unwrap_or_else(|| root.to_owned()),
                    message: error.to_string(),
                    repaired: false,
                });
                snapshot
            }
        };
        LanguageLoad {
            snapshot,
            diagnostics,
        }
    }

    /// Construct a fallback without any filesystem access, including on first use.
    pub fn embedded(generation: u64) -> Self {
        let sources = embedded_sources();
        Self::from_sources(
            DEFAULT_LANGUAGE.to_owned(),
            generation,
            Vec::new(),
            sources.clone(),
            sources,
        )
        .unwrap_or_else(|_| {
            // Invalid build-time resources must not make the recovery UI panic.
            let sources = vec![Source {
                path: PathBuf::from("<built-in>"),
                text: include_str!("emergency.ftl").to_owned(),
            }];
            Self::from_sources(
                DEFAULT_LANGUAGE.to_owned(),
                generation,
                Vec::new(),
                sources.clone(),
                sources,
            )
            .expect("static emergency Fluent resource is valid")
        })
    }

    fn load_inner(
        root: &Path,
        code: &str,
        generation: u64,
        lenient: bool,
        diagnostics: &mut Vec<RepairDiagnostic>,
    ) -> Result<Self, LanguageError> {
        let code = canonical_language_code(code)?;
        let embedded = embedded_sources();
        parse_manifest(
            EMBEDDED_MANIFEST,
            DEFAULT_LANGUAGE,
            Path::new("<embedded>/manifest.toml"),
        )?;
        resource::validate(&embedded, None, true)?;
        let english = repair_english(root, &embedded, diagnostics);
        let mut current = Vec::new();
        if code != DEFAULT_LANGUAGE {
            let directory = root.join("locales").join(&code);
            let path = directory.join("manifest.toml");
            let manifest = fs::read_to_string(&path).map_err(|error| {
                LanguageError::new(
                    LanguageErrorKind::Manifest,
                    Some(path.clone()),
                    error.to_string(),
                )
            })?;
            parse_manifest(&manifest, &code, &path)?;
            current = if lenient {
                let (sources, errors) = resource::read_sources_tolerant(&directory);
                diagnostics.extend(errors.into_iter().map(|error| RepairDiagnostic {
                    kind: RepairKind::InvalidResource,
                    path: error.path.unwrap_or_else(|| directory.clone()),
                    message: error.message,
                    repaired: false,
                }));
                sources
            } else {
                resource::read_sources(&directory)?
            };
            loop {
                match resource::validate(&current, Some(&english), true) {
                    Ok(_) => break,
                    Err(error) if lenient => {
                        let before = current.len();
                        current.retain(|source| Some(&source.path) != error.path.as_ref());
                        diagnostics.push(RepairDiagnostic {
                            kind: RepairKind::InvalidResource,
                            path: error.path.clone().unwrap_or_else(|| directory.clone()),
                            message: error.to_string(),
                            repaired: false,
                        });
                        if current.len() == before {
                            current.clear();
                            break;
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        if code != DEFAULT_LANGUAGE {
            let english_contracts = resource::validate(&english, None, true)?;
            let current_patterns = resource::validate(&current, None, false)?;
            let missing: Vec<_> = english_contracts
                .keys()
                .filter(|id| !id.starts_with('-') && !current_patterns.contains_key(*id))
                .collect();
            if !missing.is_empty() {
                diagnostics.push(RepairDiagnostic {
                    kind: RepairKind::MissingTranslations,
                    path: root.join("locales").join(&code),
                    message: format!(
                        "{} messages fall back to English; examples: {}",
                        missing.len(),
                        missing
                            .iter()
                            .take(8)
                            .map(|id| id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    repaired: false,
                });
            }
        }
        Self::from_sources(code, generation, current, english, embedded)
    }

    fn from_sources(
        code: String,
        generation: u64,
        current_sources: Vec<Source>,
        english_sources: Vec<Source>,
        embedded_sources: Vec<Source>,
    ) -> Result<Self, LanguageError> {
        let embedded = make_bundle(DEFAULT_LANGUAGE, &embedded_sources, &[])?;
        let english = make_bundle(DEFAULT_LANGUAGE, &embedded_sources, &english_sources)?;
        let current = make_bundle(&code, &current_sources, &[])?;
        let emergency = make_bundle(
            DEFAULT_LANGUAGE,
            &[Source {
                path: "<emergency>".into(),
                text: include_str!("emergency.ftl").to_owned(),
            }],
            &[],
        )?;
        Ok(Self {
            code,
            generation,
            current,
            english,
            embedded,
            emergency,
            current_sources,
            english_sources,
            embedded_sources,
        })
    }

    /// Format current → disk English → embedded English. Resolver errors trigger the next tier.
    /// An unknown ID remains visible as `[id]` rather than leaking a partially formatted pattern.
    pub fn render(&self, message: &LocalizedMessage) -> String {
        self.render_at_depth(message, 0)
    }

    fn render_at_depth(&self, message: &LocalizedMessage, depth: usize) -> String {
        if depth >= 32 {
            let emergency = self
                .emergency
                .get_message("i18n-unavailable")
                .and_then(|message| message.value())
                .expect("static emergency message exists");
            return self
                .emergency
                .format_pattern(emergency, None, &mut Vec::new())
                .into_owned();
        }
        let mut args = FluentArgs::new();
        for (name, value) in &message.args {
            match value {
                MessageArg::String(value) => args.set(name, value.as_str()),
                MessageArg::Integer(value) => args.set(name, *value),
                MessageArg::Message(value) => {
                    args.set(name, self.render_at_depth(value, depth + 1))
                }
            }
        }
        for bundle in [
            &self.current,
            &self.english,
            &self.embedded,
            &self.emergency,
        ] {
            let (id, attribute) = message
                .id
                .split_once('.')
                .map_or((message.id.as_str(), None), |(id, attribute)| {
                    (id, Some(attribute))
                });
            let Some(entry) = bundle.get_message(id) else {
                continue;
            };
            let pattern = match attribute {
                Some(attribute) => entry
                    .get_attribute(attribute)
                    .map(|attribute| attribute.value()),
                None => entry.value(),
            };
            let Some(pattern) = pattern else {
                continue;
            };
            let mut errors = Vec::new();
            let rendered = bundle.format_pattern(pattern, Some(&args), &mut errors);
            if errors.is_empty() {
                return rendered.into_owned();
            }
        }
        format!("[{}]", message.id)
    }

    pub fn render_text(&self, text: &LocalizedText) -> String {
        match text {
            LocalizedText::Message(message) => self.render(message),
            LocalizedText::Raw(raw) => raw.clone(),
        }
    }
}

fn embedded_sources() -> Vec<Source> {
    EMBEDDED_FILES
        .iter()
        .map(|(path, text)| Source {
            path: PathBuf::from(path),
            text: (*text).to_owned(),
        })
        .collect()
}

fn make_bundle(code: &str, base: &[Source], overrides: &[Source]) -> Result<Bundle, LanguageError> {
    let language = code.parse().map_err(|_| {
        LanguageError::new(
            LanguageErrorKind::UnknownLanguage,
            None,
            format!("Invalid language code: {code}"),
        )
    })?;
    let mut bundle = Bundle::new_concurrent(vec![language]);
    bundle.set_use_isolating(false);
    bundle.add_builtins().expect("new bundle has no functions");
    for (sources, override_entries) in [(base, false), (overrides, true)] {
        for source in sources {
            let resource =
                FluentResource::try_new(source.text.clone()).map_err(|(_, errors)| {
                    LanguageError::new(
                        LanguageErrorKind::Syntax,
                        Some(source.path.clone()),
                        format!("Invalid Fluent syntax: {errors:?}"),
                    )
                })?;
            if override_entries {
                bundle.add_resource_overriding(resource);
            } else {
                bundle.add_resource(resource).map_err(|errors| {
                    LanguageError::new(
                        LanguageErrorKind::Duplicate,
                        Some(source.path.clone()),
                        format!("Duplicate Fluent entries: {errors:?}"),
                    )
                })?;
            }
        }
    }
    Ok(bundle)
}

fn diagnostic_write(
    path: &Path,
    text: &str,
    kind: RepairKind,
    diagnostics: &mut Vec<RepairDiagnostic>,
) {
    match atomic_write(path, text) {
        Ok(()) => diagnostics.push(RepairDiagnostic {
            kind,
            path: path.to_owned(),
            message: format!("Restored English resource: {}", path.display()),
            repaired: true,
        }),
        Err(error) => {
            diagnostics.push(RepairDiagnostic {
                kind,
                path: path.to_owned(),
                message: "English resource restored in memory".to_owned(),
                repaired: false,
            });
            diagnostics.push(RepairDiagnostic {
                kind: RepairKind::WriteFailed,
                path: path.to_owned(),
                message: error.to_string(),
                repaired: false,
            });
        }
    }
}

fn repair_english(
    root: &Path,
    embedded: &[Source],
    diagnostics: &mut Vec<RepairDiagnostic>,
) -> Vec<Source> {
    let directory = root.join("locales").join(DEFAULT_LANGUAGE);
    let manifest = directory.join("manifest.toml");
    if fs::read_to_string(&manifest)
        .ok()
        .and_then(|source| parse_manifest(&source, DEFAULT_LANGUAGE, &manifest).ok())
        .is_none()
    {
        diagnostic_write(
            &manifest,
            EMBEDDED_MANIFEST,
            RepairKind::InvalidManifest,
            diagnostics,
        );
    }
    let (mut sources, errors) = resource::read_sources_tolerant(&directory);
    diagnostics.extend(errors.into_iter().map(|error| RepairDiagnostic {
        kind: RepairKind::InvalidResource,
        path: error.path.unwrap_or_else(|| directory.clone()),
        message: error.message,
        repaired: false,
    }));
    // Individually corrupt files cannot contribute IDs when finding missing defaults.
    let mut known: BTreeSet<String> = sources
        .iter()
        .filter(|source| resource::validate(std::slice::from_ref(source), None, false).is_ok())
        .flat_map(|source| resource::identifiers(std::slice::from_ref(source)))
        .collect();
    for default in embedded {
        let path = directory.join(&default.path);
        let index = sources.iter().position(|source| source.path == path);
        let existing = index.map(|index| sources[index].clone()).or_else(|| {
            fs::read_to_string(&path).ok().map(|text| Source {
                path: path.clone(),
                text,
            })
        });
        let (text, kind) = match existing {
            Some(source) => match resource::append_missing(&source, default, &known) {
                Ok(None) => {
                    if index.is_none() {
                        sources.push(source);
                    }
                    continue;
                }
                Ok(Some(text)) => (text, RepairKind::MissingMessages),
                Err(_) => (default.text.clone(), RepairKind::CorruptFile),
            },
            None => {
                // Entries moved into another healthy file must not be duplicated by repair.
                let empty = Source {
                    path: path.clone(),
                    text: String::new(),
                };
                let text = resource::append_missing(&empty, default, &known)
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                (
                    text,
                    if path.exists() {
                        RepairKind::CorruptFile
                    } else {
                        RepairKind::MissingFile
                    },
                )
            }
        };
        diagnostic_write(&path, &text, kind, diagnostics);
        let source = Source { path, text };
        known.extend(resource::identifiers(std::slice::from_ref(&source)));
        if let Some(index) = index {
            sources[index] = source;
        } else {
            sources.push(source);
        }
    }
    // Repair semantic errors (references, duplicate IDs, and changed argument contracts) too.
    let budget = sources.len() + embedded.len() + 1;
    for _ in 0..budget {
        match resource::validate(&sources, Some(embedded), true) {
            Ok(_) => {
                // Ensure in-memory English remains complete after excluding a bad optional file.
                let known = resource::identifiers(&sources);
                for default in embedded {
                    let empty = Source {
                        path: default.path.clone(),
                        text: String::new(),
                    };
                    if let Ok(Some(text)) = resource::append_missing(&empty, default, &known) {
                        sources.push(Source {
                            path: PathBuf::from("<embedded>").join(&default.path),
                            text,
                        });
                    }
                }
                sources.sort_by(|a, b| a.path.cmp(&b.path));
                return sources;
            }
            Err(error) => {
                let index = sources
                    .iter()
                    .position(|source| Some(&source.path) == error.path.as_ref());
                let Some(index) = index else {
                    break;
                };
                let path = sources[index].path.clone();
                let default = embedded
                    .iter()
                    .find(|default| directory.join(&default.path) == path);
                if let Some(default) = default.filter(|default| default.text != sources[index].text)
                {
                    diagnostic_write(&path, &default.text, RepairKind::CorruptFile, diagnostics);
                    sources[index].text = default.text.clone();
                } else {
                    diagnostics.push(RepairDiagnostic {
                        kind: RepairKind::InvalidResource,
                        path,
                        message: error.to_string(),
                        repaired: false,
                    });
                    sources.remove(index);
                }
            }
        }
    }
    diagnostics.push(RepairDiagnostic {
        kind: RepairKind::StartupFallback,
        path: directory,
        message: "English validation failed; using embedded defaults in memory".to_owned(),
        repaired: false,
    });
    embedded.to_vec()
}

fn atomic_write(path: &Path, text: &str) -> std::io::Result<()> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("Missing parent directory"))?;
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".i18n-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    let result = (|| {
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
