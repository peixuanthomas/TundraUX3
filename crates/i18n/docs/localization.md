# Language resources and recovery

TundraUX3 provides `en-US` and `zh-CN`, with `en-US` as the default language. A `zh-Hans` setting is normalized to `zh-CN`. Language and visual theme are saved separately.

## Switching and reloading

Select and confirm a language in Region & Time. Selecting the current language again also rereads the catalog and resources, allowing updated translations to load.

Only after loading, validation, and configuration saving all succeed does Shell publish the new immutable snapshot in a single step. On failure, the current language and configuration are retained. After confirming the language during initial setup, the rest of the wizard uses that language.

`tundra-cli config set language zh-CN` uses the same catalog and validation. After a CLI configuration change, an existing Shell reads it at its next startup or when the language is confirmed again.

## Directory structure

Source resources reside in `crates/ascii-assets/assets/locales`; release resources reside in `assets/locales`, alongside `themes`.

Each language pack contains:

```text
<language>/
  manifest.toml
  common/
  modules/
  recovery/
```

Example `manifest.toml`:

```toml
format_version = 1
code = "zh-CN"
native_name = "简体中文"
```

The directory name and `code` use the canonical language code. `.ftl` files are organized by shared components, application modules, and startup recovery.

## Writing messages

Use stable message IDs, complete sentences, and named arguments:

```ftl
files-copied =
    { $count ->
        [one] Copied one file to { $destination }.
       *[other] Copied { $count } files to { $destination }.
    }
```

Translations may use different plural branches, but their argument sets must match the English contracts. Missing translations fall back to English. `i18n::ids`, `MESSAGE_IDS`, and `MESSAGE_CONTRACTS` are generated at build time.

In Rust, use `i18n::msg!` to retain messages and `i18n::tr!` to format them in view models or during rendering. `LocalizedText::Raw` carries raw external text.

Commands, actions, and icons are identified by stable IDs or enums, not translated labels. Configuration keys, protocol fields, paths, user input, third-party output, and raw diagnostics remain unchanged. Logs use stable event codes and descriptions in a fixed language.

## Default-resource repair

Default English, Chinese, and graphical resources are checked at startup and on reload:

- Missing or damaged default files are restored from built-in content, with atomic replacement through temporary files in the same directory.
- Healthy custom resources remain unchanged; missing default entries can be appended to existing files.
- When the resource directory is not writable, in-memory copies are used and the saved language preference remains unchanged.
- Startup repair results are shown together; repairs during language reload are reported as part of that operation.

The recovery interface offers "Repair automatically and restart" and "Exit", saving editor recovery data before executing the action.

## Validation

```sh
cargo test --locked -p i18n
python3 scripts/check-localization.py
```

Shell integration tests cover same-language reloads, rollback on failure, catalog discovery, and configuration write failures. UI tests cover Chinese character widths, long text, and hit regions. Release packages must include the complete `assets/` directory.
