# i18n

`i18n` handles Fluent language-pack loading, validation, default-resource repair, and immutable language snapshots. It shares a resource root with `ascii-assets`; language packs reside in `root/locales/<code>/`.

## Language packs

Each language pack contains a `manifest.toml`:

```toml
format_version = 1
code = "zh-CN"
native_name = "简体中文"
```

`.ftl` files in `common`, `modules`, and `recovery` are loaded recursively. `zh-Hans` is an input alias for `zh-CN`; directories and manifests use the canonical code. `LanguageCatalog::built_in()` provides English and Chinese language choices without accessing the filesystem.

The bundled Chinese and English packs include compact-interface preview descriptions, primary and danger action labels, and terminal-size messages with interpolated minimum dimensions. Keep both languages and their argument contracts in sync when changing these shared UI resources.

The bundled `modules/workspace.ftl` covers the Windows panel, window states and size requirements, close and cleanup prompts, and watchdog CLI help and errors. CLI flags and JSON status values are shared across languages.

## Snapshots and messages

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

- `msg!` retains a message ID and typed arguments; `tr!` formats immediately.
- `enter_snapshot` returns a thread-bound RAII guard. Scopes can nest and are isolated between threads.
- `current_snapshot()` clones the current thread's snapshot; background helpers can transfer language resources through `export()` / `restore()`.
- `String` and `&str` always represent raw text when converted to `LocalizedText` and are not translated.
- Rendering reads only memory, without disk access or log writes.

Snapshots separately retain the current language, English fallback, built-in English, and minimal emergency resources. Missing messages or formatting failures fall back through these layers; unknown IDs appear as `[message-id]`.

## Loading and validation

Strict loading checks manifests, Fluent syntax, duplicate IDs, references and cycles, functions, and English argument contracts. `NUMBER` is supported. Missing translations are recorded in loading diagnostics and fall back to English.

Startup loading can skip damaged translation files and continue with built-in English and Chinese resources. Existing snapshots remain unchanged when disk files change; after a successful reload, the caller publishes a new snapshot.

The build script embeds English from `crates/ascii-assets/assets/locales/en-US` and generates:

- `i18n::ids` message constants;
- `MESSAGE_IDS`;
- `MESSAGE_CONTRACTS` and `message_contract(id)`.

Default English resources and damaged English or Chinese files are repaired using temporary files in the same directory and atomic renames. Healthy files retain their original bytes, missing entries are appended to the appropriate files, and write failures retain the in-memory corrections and return structured diagnostics.

`render_diagnostic` always uses built-in English and is suitable for error `Display` implementations and logs. `LocalizedError` retains a stable `event_code`, a message, and an optional cause chain.

## Documentation and validation

See [Language resources and recovery](docs/localization.md) for usage and resource recovery.

```sh
cargo test --locked -p i18n
python3 scripts/check-localization.py
```

Tests cover catalogs, repair, argument contracts, plural fallback, snapshot lifetimes, thread isolation, and serialization.
