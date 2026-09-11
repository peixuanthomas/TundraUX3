/// A canonical English message pattern and its required named arguments.
/// Both tables and argument names are sorted for deterministic lookup and tooling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessageContract {
    pub id: &'static str,
    pub args: &'static [&'static str],
}

/// Look up the compiled English contract without reading disk or consulting UI state.
pub fn message_contract(id: &str) -> Option<&'static MessageContract> {
    crate::MESSAGE_CONTRACTS
        .binary_search_by_key(&id, |contract| contract.id)
        .ok()
        .map(|index| &crate::MESSAGE_CONTRACTS[index])
}
