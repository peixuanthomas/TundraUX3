# ascii-assets

`ascii-assets` loads, validates, and repairs themes, ASCII artwork, icons, fonts, and language resource files, and calculates the dimensions needed for rendering. [i18n](../i18n/README.md) handles language loading and translation.

## Modules

| Module | Responsibility |
| --- | --- |
| `artwork` | Text artwork, art sets, and Home/Launcher/file icon types |
| `asset_manifest` | Required asset manifest, asset types, and default theme ID |
| `asset_resolver` | Resolve asset roots from environment variables, the program directory, and installation locations |
| `asset_store` | Asset loading and rendering-dimension statistics |
| `asset_validation` | Required asset and default theme checks |
| `asset_restore` | Restore asset files from embedded defaults |
| `asset_distribution` | Copy canonical assets to the Cargo output directory |
| `embedded_defaults` | Shared embedded default files and image bytes |
| `clock_font` | Clock font assets |

## Asset formats

The default manifest contains 25 items: 20 text assets, 4 TOML art sets, and 1 clock font.

- Text assets contain only printable ASCII; TOML art sets use schema v1.
- Images support GIF, JPEG, PNG, and WebP; paths must be relative paths within the asset root.
- Language resources live in `locales/<code>/`, containing `manifest.toml` and recursively read `.ftl` files.
- Default image bytes are shared through a static asset table; validation, repair, and in-memory fallback reuse the same data.
- All assets contribute to dimension calculations; updating assets may change the minimum terminal size.

Asset roots are resolved in this order:

1. `TUNDRA_ASCII_ASSETS_DIR`;
2. `assets/` alongside the program;
3. `/usr/share/tundraux3/assets` on Linux.

## Default asset repair

Startup checks default graphics and the Chinese and English language resources. Missing or corrupt default files are restored from embedded content, while healthy custom content remains unchanged. Older default-theme configurations can gain new entries without removing the user's existing artwork.

If the asset directory cannot be written, the program uses in-memory copies and reports the repair result. See [Language resources and recovery](../i18n/docs/localization.md) for language message IDs, argument contracts, and fallback rules.

## Validation

```sh
cargo test --locked -p ascii-assets
python scripts/check-embedded-assets.py target/release/tundra-shell.exe target/release/tundra-cli.exe
```

On Linux/macOS, use program paths without `.exe`. The check script verifies that each default image is embedded exactly once; if a platform does not use images at all, add `--allow-missing` to check only for duplicates.

Asset files are in `assets/`; see [Debug interface previews](../shell/docs/previews.md) for interface preview commands.
