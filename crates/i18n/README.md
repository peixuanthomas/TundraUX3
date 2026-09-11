# i18n

The `i18n` crate owns Fluent loading, validation, English repair, and immutable
language snapshots. Its root argument is the same **asset root** used by
`ascii-assets`: language packs live at `root/locales/<code>/`.

Each pack contains `manifest.toml`:

```toml
format_version = 1
code = "zh-CN"
native_name = "简体中文"
```

All `.ftl` files below the pack are read recursively (including `common`,
`modules`, and `recovery`). `zh-Hans` is an input alias for `zh-CN`; manifests
and directory names use canonical codes. Catalog discovery returns valid packs
and diagnostics for invalid manifests. Embedded English is always listed.
`LanguageCatalog::built_in()` returns English/Chinese selector metadata without
filesystem access.

```rust
use std::sync::Arc;
use i18n::{LanguageSnapshot, LocalizedText};

let load = LanguageSnapshot::load_startup("assets", "zh-CN", 1);
let snapshot = Arc::new(load.snapshot);
i18n::with_snapshot(&snapshot, || {
    let label = i18n::tr!("notifications-action-ok");
    let retained = i18n::msg!("resources-repaired", count = 1, files = "manifest.toml");
    let raw = LocalizedText::from("external command output");
    assert_eq!(raw.render_current(), "external command output");
});
```

`enter_snapshot(Arc<LanguageSnapshot>)` returns a thread-bound RAII guard for
larger render/input scopes. Scopes nest, restore after unwinding, and are isolated
between threads. Formatting outside a scope lazily initializes embedded English
in memory. `msg!` retains a message ID and typed, serializable arguments; `tr!`
formats immediately. Nested `LocalizedMessage` and `LocalizedText` arguments retain
their original IDs and render through the same snapshot; nesting stops at depth
32 with an emergency fallback. `String`/`&str` conversions to `LocalizedText` are always raw.

`LanguageSnapshot::load` is strict for the requested pack. It rejects malformed
manifests, invalid Fluent, duplicate message/term/attribute IDs, unresolved or
cyclic references, unsupported functions, and changed named-argument contracts
relative to English. Message contracts include arguments inherited through
references. `NUMBER` is supported. Missing translations are allowed and reported
once in load diagnostics. Startup loading additionally skips bad translation
files and falls back to English when the requested locale cannot load.

Each snapshot owns separate current-language, English, embedded-English, and
minimal emergency-recovery bundles. Missing messages and formatting failures fall
through these bundles in order. Each tier uses its own locale's plural rules. A
translated message whose reference cannot resolve falls back as a whole. Unknown
IDs or unsatisfied arguments yield `[message-id]`. Rendering performs no file I/O
or logging. Snapshots implement `Send + Sync`, `Debug`, and content-aware equality.

English defaults are embedded from the canonical
`../ascii-assets/assets/locales/en-US` tree. The build script watches directories
and files recursively and validates the embedded manifest and Fluent reference
graph. Before canonical files exist, an internal recovery fixture bootstraps the
crate. Minimal recovery messages are also always available as the last tier.

Loading repairs missing or corrupt default English files via a same-directory
temporary file and atomic rename. Healthy files retain their bytes; missing
entries are appended while retaining existing entries/comments. Entries moved to
another healthy file are not duplicated. Write failures leave the corrected
content in memory and produce structured diagnostics:

- `kind: RepairKind`
- `path: PathBuf`
- `message: String`
- `repaired: bool` (whether the correction was persisted)

`LanguageLoad` exposes `snapshot` and `diagnostics`. Callers own logging, recovery
presentation, persistence of the selected language, and publication of a new
`Arc<LanguageSnapshot>` after a successful strict reload. Existing snapshots
remain unchanged even if asset files are edited or removed.

Use `render_diagnostic(&message)` for domain error `Display` implementations and
log text. It always uses a lazily initialized immutable embedded English snapshot,
including for nested arguments, independently of the active UI scope and editable
English files. Both initialization and rendering are free of filesystem access.

`LocalizedError` carries a stable `event_code`, a retained `LocalizedMessage`, and
an optional boxed `LocalizedError` cause. Its `Display` is machine-oriented;
render its message explicitly for user-facing presentation.

Run `cargo test -p i18n` for catalog, repair, contracts, plural fallback, partial
startup recovery, snapshot lifetime, thread isolation, and serialization checks.
