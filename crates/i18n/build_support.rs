use std::collections::{BTreeMap, BTreeSet};

/// Generate public names and argument contracts from validated English patterns.
pub(crate) fn generate_catalog(
    contracts: &BTreeMap<String, BTreeSet<String>>,
) -> Result<String, String> {
    let messages: Vec<_> = contracts
        .iter()
        .filter(|(id, _)| !id.starts_with('-'))
        .collect();
    if messages.is_empty() {
        return Err("canonical English pack must contain at least one message".to_owned());
    }
    let mut names = BTreeMap::new();
    let mut constants = String::from(
        "/// Message IDs generated from canonical English resources.\npub mod ids {\n",
    );
    let mut rows = String::new();
    for (id, args) in &messages {
        let name = id.replace(['-', '.'], "_").to_ascii_uppercase();
        if let Some(previous) = names.insert(name.clone(), id.as_str()) {
            return Err(format!(
                "English message IDs {previous:?} and {id:?} collide as ids::{name}"
            ));
        }
        constants.push_str(&format!("pub const {name}: &str = {id:?};\n"));
        rows.push_str(&format!(
            "crate::MessageContract {{ id: {id:?}, args: &{:?} }},\n",
            args.iter().collect::<Vec<_>>()
        ));
    }
    constants.push_str("}\n");
    let ids: Vec<_> = messages.iter().map(|(id, _)| id.as_str()).collect();
    Ok(format!(
        "{constants}\n/// Sorted canonical English message pattern IDs.\npub const MESSAGE_IDS: &[&str] = &{ids:?};\n\n/// Named argument contracts generated from canonical English, including referenced arguments.\npub const MESSAGE_CONTRACTS: &[crate::MessageContract] = &[\n{rows}];\n"
    ))
}
