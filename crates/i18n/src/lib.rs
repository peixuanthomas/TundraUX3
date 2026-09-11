//! Immutable Fluent localization with a shared asset root and embedded recovery.
mod catalog;
mod error;
mod message;
mod resource;
mod scope;
mod snapshot;

pub use catalog::canonical_language_code as canonicalize_locale;
pub use catalog::{
    DEFAULT_LANGUAGE, LanguageCatalog, LanguageOption, canonical_language_code, default_asset_root,
};
pub use error::{LanguageError, LanguageErrorKind, RepairDiagnostic, RepairKind};
pub use message::{LocalizedError, LocalizedMessage, LocalizedText, MessageArg};
pub use scope::{SnapshotGuard, enter_snapshot, render_current, render_diagnostic, with_snapshot};
pub use snapshot::{LanguageLoad, LanguageSnapshot};

include!(concat!(env!("OUT_DIR"), "/embedded.rs"));

/// Build a retained, typed message; argument expressions are evaluated once.
#[macro_export]
macro_rules! msg {
    ($id:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::LocalizedMessage::new($id)$(.with_arg(stringify!($name), $value))*
    };
}

/// Format using the thread's active immutable snapshot (embedded English by default).
#[macro_export]
macro_rules! tr {
    ($id:expr $(, $name:ident = $value:expr)* $(,)?) => {
        $crate::render_current(&$crate::msg!($id $(, $name = $value)*))
    };
}

#[cfg(test)]
mod tests;
