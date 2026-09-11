use std::{env, fs, path::Path};
mod build_support;
#[allow(dead_code)]
#[path = "src/error.rs"]
mod error;
use error::{LanguageError, LanguageErrorKind};
#[allow(dead_code)]
#[path = "src/resource.rs"]
mod resource;

fn walk(root: &Path, path: &Path, files: &mut Vec<String>) {
    println!("cargo:rerun-if-changed={}", path.display());
    let mut entries: Vec<_> = fs::read_dir(path)
        .expect("read canonical English locale directory")
        .map(|entry| entry.expect("read canonical locale entry").path())
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            walk(root, &entry, files);
        } else if entry
            .extension()
            .is_some_and(|extension| extension == "ftl")
        {
            println!("cargo:rerun-if-changed={}", entry.display());
            let relative = entry.strip_prefix(root).unwrap().to_str().unwrap();
            files.push(format!(
                "({relative:?}, include_str!({:?}))",
                entry.to_str().unwrap()
            ));
        }
    }
}

fn main() {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../ascii-assets/assets/locales/en-US");
    // Track additions/removals as well as edits to existing resources.
    println!(
        "cargo:rerun-if-changed={}",
        root.parent().unwrap().display()
    );
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    let manifest = root.join("manifest.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let manifest_source = fs::read_to_string(&manifest)
        .expect("canonical English manifest must exist and be readable");
    let parsed: toml::Value =
        toml::from_str(&manifest_source).expect("embedded English manifest must be valid TOML");
    assert_eq!(
        parsed
            .get("format_version")
            .and_then(toml::Value::as_integer),
        Some(1),
        "embedded English manifest requires format_version=1"
    );
    assert_eq!(
        parsed.get("code").and_then(toml::Value::as_str),
        Some("en-US"),
        "embedded English manifest requires code=en-US"
    );
    assert!(
        parsed
            .get("native_name")
            .and_then(toml::Value::as_str)
            .is_some_and(|name| !name.trim().is_empty()),
        "embedded English manifest requires native_name"
    );
    let sources = resource::read_sources(&root).expect("read canonical English resources");
    let contracts = resource::validate(&sources, None, true).expect(
        "embedded English resources must have valid syntax, unique IDs, and valid references",
    );
    let public_catalog = build_support::generate_catalog(&contracts)
        .expect("canonical English message constants and contracts must be valid");
    let emergency = resource::Source {
        path: "src/emergency.ftl".into(),
        text: include_str!("src/emergency.ftl").to_owned(),
    };
    resource::validate(&[emergency], None, true).expect("minimal recovery resources must be valid");
    println!("cargo:rerun-if-changed=src/emergency.ftl");
    println!("cargo:rerun-if-changed=src/resource.rs");
    println!("cargo:rerun-if-changed=src/error.rs");
    println!("cargo:rerun-if-changed=build_support.rs");
    let manifest_expr = format!("include_str!({:?})", manifest.to_str().unwrap());
    let output = format!(
        "pub(crate) const EMBEDDED_MANIFEST: &str = {manifest_expr};\npub(crate) const EMBEDDED_FILES: &[(&str, &str)] = &[{}];\n{public_catalog}",
        files.join(",\n")
    );
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("embedded.rs"),
        output,
    )
    .expect("write embedded locale index");
}
