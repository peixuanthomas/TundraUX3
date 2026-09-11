use crate::{LanguageError, LanguageErrorKind};
use fluent_syntax::{ast, parser, serializer};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Source {
    pub path: PathBuf,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
struct Node {
    args: BTreeSet<String>,
    references: Vec<(String, BTreeSet<String>)>,
    functions: BTreeSet<String>,
}

pub(crate) type Contracts = BTreeMap<String, BTreeSet<String>>;

pub(crate) fn parse(source: &Source) -> Result<ast::Resource<String>, LanguageError> {
    parser::parse(source.text.clone()).map_err(|(_, errors)| {
        LanguageError::new(
            LanguageErrorKind::Syntax,
            Some(source.path.clone()),
            format!("Invalid Fluent syntax: {errors:?}"),
        )
    })
}

pub(crate) fn entry_id(entry: &ast::Entry<String>) -> Option<String> {
    match entry {
        ast::Entry::Message(message) => Some(message.id.name.clone()),
        ast::Entry::Term(term) => Some(format!("-{}", term.id.name)),
        _ => None,
    }
}

pub(crate) fn append_missing(
    source: &Source,
    default: &Source,
    existing: &BTreeSet<String>,
) -> Result<Option<String>, LanguageError> {
    // Check duplicates even if no additions are necessary.
    validate(std::slice::from_ref(source), None, false)?;
    let mut parsed = parse(default)?;
    parsed
        .body
        .retain(|entry| entry_id(entry).is_some_and(|id| !existing.contains(&id)));
    if parsed.body.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "{}\n{}",
        source.text,
        serializer::serialize(&parsed)
    )))
}

pub(crate) fn identifiers(sources: &[Source]) -> BTreeSet<String> {
    sources
        .iter()
        .filter_map(|source| parse(source).ok())
        .flat_map(|resource| {
            resource
                .body
                .into_iter()
                .filter_map(|entry| entry_id(&entry))
        })
        .collect()
}

/// Validate the complete graph, including cross-file references and transitive arguments.
/// For translations, fallback nodes fill omissions; explicit nodes retain exact English contracts.
pub(crate) fn validate(
    sources: &[Source],
    fallback: Option<&[Source]>,
    resolve: bool,
) -> Result<Contracts, LanguageError> {
    let mut nodes = BTreeMap::new();
    let mut owners = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for source in sources {
        for entry in parse(source)?.body {
            if let Some(id) = entry_id(&entry)
                && !ids.insert(id.clone())
            {
                return Err(LanguageError::new(
                    LanguageErrorKind::Duplicate,
                    Some(source.path.clone()),
                    format!("Duplicate Fluent entry: {id}"),
                ));
            }
            let (id, value, attributes) = match entry {
                ast::Entry::Message(message) => {
                    (message.id.name, message.value, message.attributes)
                }
                ast::Entry::Term(term) => (
                    format!("-{}", term.id.name),
                    Some(term.value),
                    term.attributes,
                ),
                _ => continue,
            };
            let mut add =
                |key: String, pattern: &ast::Pattern<String>| -> Result<(), LanguageError> {
                    let mut node = Node::default();
                    visit_pattern(pattern, &mut node);
                    if nodes.insert(key.clone(), node).is_some() {
                        return Err(LanguageError::new(
                            LanguageErrorKind::Duplicate,
                            Some(source.path.clone()),
                            format!("Duplicate Fluent pattern: {key}"),
                        ));
                    }
                    owners.insert(key, source.path.clone());
                    Ok(())
                };
            if let Some(value) = value {
                add(id.clone(), &value)?;
            }
            for attribute in attributes {
                add(format!("{id}.{}", attribute.id.name), &attribute.value)?;
            }
        }
    }
    let explicit: BTreeSet<_> = nodes.keys().cloned().collect();
    if let Some(fallback) = fallback {
        // Parse fallback through the same collector; its resolved contracts are checked separately.
        for source in fallback {
            for entry in parse(source)?.body {
                let (id, value, attributes) = match entry {
                    ast::Entry::Message(message) => {
                        (message.id.name, message.value, message.attributes)
                    }
                    ast::Entry::Term(term) => (
                        format!("-{}", term.id.name),
                        Some(term.value),
                        term.attributes,
                    ),
                    _ => continue,
                };
                let mut add = |key: String, pattern: &ast::Pattern<String>| {
                    // Fluent replaces whole entries, so attributes of translated messages cannot fall through.
                    if !ids.contains(&id) {
                        let mut node = Node::default();
                        visit_pattern(pattern, &mut node);
                        owners.insert(key.clone(), source.path.clone());
                        nodes.entry(key).or_insert(node);
                    }
                };
                if let Some(value) = value {
                    add(id.clone(), &value);
                }
                for attribute in attributes {
                    add(format!("{id}.{}", attribute.id.name), &attribute.value);
                }
            }
        }
    }
    let mut contracts = BTreeMap::new();
    for id in &explicit {
        let args = if resolve {
            resolve_args(id, &nodes, &mut BTreeSet::new(), &mut contracts).map_err(
                |(kind, message)| LanguageError::new(kind, owners.get(id).cloned(), message),
            )?
        } else {
            nodes[id].args.clone()
        };
        contracts.insert(id.clone(), args);
    }
    if let Some(fallback) = fallback {
        let english = validate(fallback, None, true)?;
        for id in &explicit {
            if id.starts_with('-') {
                continue;
            }
            if let Some(expected) = english.get(id)
                && contracts[id] != *expected
            {
                return Err(LanguageError::new(
                    LanguageErrorKind::Parameters,
                    owners.get(id).cloned(),
                    format!(
                        "Argument contract mismatch for {id}: expected {expected:?}, found {:?}",
                        contracts[id]
                    ),
                ));
            }
        }
    }
    Ok(contracts)
}

fn resolve_args(
    id: &str,
    nodes: &BTreeMap<String, Node>,
    visiting: &mut BTreeSet<String>,
    cache: &mut Contracts,
) -> Result<BTreeSet<String>, (LanguageErrorKind, String)> {
    if let Some(args) = cache.get(id) {
        return Ok(args.clone());
    }
    let node = nodes.get(id).ok_or_else(|| {
        (
            LanguageErrorKind::Reference,
            format!("Unknown Fluent reference: {id}"),
        )
    })?;
    if !visiting.insert(id.to_owned()) {
        return Err((
            LanguageErrorKind::Reference,
            format!("Cyclic Fluent reference: {id}"),
        ));
    }
    if let Some(function) = node
        .functions
        .iter()
        .find(|function| function.as_str() != "NUMBER")
    {
        return Err((
            LanguageErrorKind::Reference,
            format!("Unsupported Fluent function: {function}"),
        ));
    }
    let mut args = node.args.clone();
    for (target, bound) in &node.references {
        let inherited = resolve_args(target, nodes, visiting, cache)?;
        args.extend(inherited.difference(bound).cloned());
    }
    visiting.remove(id);
    cache.insert(id.to_owned(), args.clone());
    Ok(args)
}

fn visit_pattern(pattern: &ast::Pattern<String>, node: &mut Node) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            visit_expression(expression, node);
        }
    }
}
fn visit_expression(expression: &ast::Expression<String>, node: &mut Node) {
    match expression {
        ast::Expression::Inline(expression) => visit_inline(expression, node),
        ast::Expression::Select { selector, variants } => {
            visit_inline(selector, node);
            for variant in variants {
                visit_pattern(&variant.value, node);
            }
        }
    }
}
fn visit_arguments(arguments: &ast::CallArguments<String>, node: &mut Node) {
    for argument in &arguments.positional {
        visit_inline(argument, node);
    }
    for argument in &arguments.named {
        visit_inline(&argument.value, node);
    }
}
fn reference(id: &str, attribute: &Option<ast::Identifier<String>>) -> String {
    match attribute {
        Some(attribute) => format!("{id}.{}", attribute.name),
        None => id.to_owned(),
    }
}
fn visit_inline(expression: &ast::InlineExpression<String>, node: &mut Node) {
    match expression {
        ast::InlineExpression::VariableReference { id } => {
            node.args.insert(id.name.clone());
        }
        ast::InlineExpression::MessageReference { id, attribute } => {
            node.references
                .push((reference(&id.name, attribute), BTreeSet::new()));
        }
        ast::InlineExpression::TermReference {
            id,
            attribute,
            arguments,
        } => {
            let mut bound = BTreeSet::new();
            if let Some(arguments) = arguments {
                visit_arguments(arguments, node);
                bound.extend(
                    arguments
                        .named
                        .iter()
                        .map(|argument| argument.name.name.clone()),
                );
            }
            node.references
                .push((reference(&format!("-{}", id.name), attribute), bound));
        }
        ast::InlineExpression::FunctionReference { id, arguments } => {
            node.functions.insert(id.name.clone());
            visit_arguments(arguments, node);
        }
        ast::InlineExpression::Placeable { expression } => visit_expression(expression, node),
        ast::InlineExpression::StringLiteral { .. }
        | ast::InlineExpression::NumberLiteral { .. } => {}
    }
}

pub(crate) fn read_sources(directory: &Path) -> Result<Vec<Source>, LanguageError> {
    let (sources, errors) = read_sources_tolerant(directory);
    match errors.into_iter().next() {
        Some(error) => Err(error),
        None => Ok(sources),
    }
}

pub(crate) fn read_sources_tolerant(directory: &Path) -> (Vec<Source>, Vec<LanguageError>) {
    fn walk(directory: &Path, sources: &mut Vec<Source>, errors: &mut Vec<LanguageError>) {
        let io_error = |path: &Path, error: std::io::Error| {
            LanguageError::new(
                LanguageErrorKind::Io,
                Some(path.to_owned()),
                error.to_string(),
            )
        };
        let entries = match fs::read_dir(directory) {
            Ok(entries) => entries,
            Err(error) => {
                errors.push(io_error(directory, error));
                return;
            }
        };
        let mut paths = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => paths.push(entry.path()),
                Err(error) => errors.push(io_error(directory, error)),
            }
        }
        paths.sort();
        for path in paths {
            let metadata = match fs::symlink_metadata(&path) {
                Ok(metadata) => metadata,
                Err(error) => {
                    errors.push(io_error(&path, error));
                    continue;
                }
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                walk(&path, sources, errors);
            } else if path.extension().is_some_and(|extension| extension == "ftl") {
                match fs::read_to_string(&path) {
                    Ok(text) => sources.push(Source { path, text }),
                    Err(error) => errors.push(io_error(&path, error)),
                }
            }
        }
    }
    let mut sources = Vec::new();
    let mut errors = Vec::new();
    walk(directory, &mut sources, &mut errors);
    (sources, errors)
}
