> Historical validation record: the DEB/Arch updates described here have been removed. Linux currently supports only portable-build updates.

# Ubuntu / Arch source-built package update validation

2026-09-27, local WSL2 Ubuntu 24.04.4 LTS, x86_64, ordinary user, Rust 1.98.0.

## Verified

- `cargo fmt --check`, `git diff --check`, and `python3 scripts/check-localization.py` passed.
- Windows `cargo check --workspace --locked` passed, with two existing warnings in unchanged files.
- Ubuntu `cargo build --workspace --locked` passed.
- `cargo test --locked -p app --lib ubuntu_builds_real_deb -- --ignored` used real
  `dpkg-shlibdeps` and `dpkg-deb` to verify package versions, shared-library dependencies, root file ownership, and program and resource contents.
- `ubuntu_export_current_build_for_install_smoke` exported two test DEBs from Shell/CLI
  built in the current workspace. Tests used debug binaries; production updates call the existing release build workflow.
- Real `apt-get --no-remove --no-install-recommends install` completed initial installation and an upgrade to the second
  package version. Initial installation added only `tundraux3` and the missing `xdg-utils` dependency; the upgrade changed only
  `tundraux3`. The test driver explicitly supplied `-y`; actual application calls retain the package manager's interactive confirmation.
- Both installations passed `ubuntu_verifies_installed_smoke_package`: the installed version matched the commit IDs of both
  programs, while deliberately supplying an incorrect version or commit ID caused verification to fail. Both source-built and system-installed programs ran
  `tundra-cli debug doctor`, correctly identified `SystemDeb`, and indicated initial installation and system-package upgrade respectively.

## Existing failure in the full test suite

`cargo test --workspace --locked --no-fail-fast` completed every target with only one failure:

```text
shell / account_workflows
linux_attaches_current_user_and_only_requires_first_appearance_setup
left: Language
right: Appearance
```

Running the same test separately in the pre-change `89e49f2` source snapshot produced the same failure.
Neither this test nor the first-start workflow was changed in this round; all other workspace tests passed.
To control WSL memory use, builds used `CARGO_BUILD_JOBS=2`,
`CARGO_PROFILE_DEV_DEBUG=0`, and `CARGO_PROFILE_TEST_DEBUG=0`.

## Not yet verified

- No Arch test system was available locally, so no real makepkg/pacman installation was performed.
  An Arch CI job and the `arch_builds_real_pacman_package` test were added; CI had not yet run.
- Manual sudo password entry, authentication cancellation, and the post-restart screen were not verified in the full interface.
  Real installations in this round were initiated by test commands; installation detection, packaging, and post-installation checks called the application's code.
- This round did not perform an end-to-end update combining network download, a full release build, and UI interaction.

Packaging-tool references: [dpkg-deb](https://manpages.debian.org/bookworm/dpkg/dpkg-deb.1.en.html),
[makepkg](https://man.archlinux.org/man/makepkg.8),
[pacman](https://man.archlinux.org/man/pacman.8).
