use std::sync::Arc;

/// Host-owned formatting against an immutable language snapshot. Weathr passes
/// message identifiers and owned named arguments; it never loads locale files.
pub type LocalizationProvider = Arc<dyn Fn(&str, &[(&str, String)]) -> String + Send + Sync>;

macro_rules! localize {
    ($provider:expr, $id:expr $(, $name:ident = $value:expr)* $(,)?) => {
        ($provider)($id, &[$((stringify!($name), ($value).to_string())),*])
    };
}
pub(crate) use localize;

#[cfg(test)]
#[path = "../tests/unit/localization/tests.rs"]
pub(crate) mod tests;
