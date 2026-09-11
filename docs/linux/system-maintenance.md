# Verified updates and explicit legacy migration

`system-maintenance` is the fixed-policy root maintenance library and the
`tundra-system-maintenance` package-managed executable. It does not run downloaded
shell scripts or source-built update helpers.

## Update integration contract

* Install the bootstrap maintenance executable at
  `/usr/libexec/tundra/tundra-system-maintenance`, owned by root and not writable by
  any other account. Keep it package-managed. Install `curl` and a recent GitHub CLI
  at `/usr/bin/curl` and `/usr/bin/gh`. GitHub CLI must support `--source-ref`,
  `--source-digest`, `--signer-digest`, and `--custom-trusted-root`.
* Provision `/etc/tundra/update-trusted-root.jsonl` with trusted Sigstore/GitHub
  verification roots through the OS package. Never take trust roots from the
  release envelope or the user's environment. Missing/expired roots fail closed;
  refresh roots through the trusted OS package distribution path.
* Seed `/var/lib/tundra/runtime/versions/v1.3.0` from the original package, including
  `bin/` and `release.json`. Runtime binaries include the private libseat-enabled
  `kmscon`, its sibling `kmscon-capabilities.json`, and the Pango module in
  `share/tundra/kmscon-modules`. The fixed module path resolves through
  `/usr/libexec/tundra/modules/kmscon`. The latter manifest has `version`, `source_sha`, `architecture`,
  `protocol`, and `runtime_sha256` fields. Bootstrap `current` is a relative
  symlink `versions/v1.3.0`. These directories are root-owned mode 0755 so ordinary
  users can execute the runtime. Staging is 0700 and control files are 0600.
* Stable launchers resolve `/var/lib/tundra/runtime/current/bin/<fixed-binary>`.
  Never replace package-owned executables during online installation. The
  maintenance executable itself is updated through OS packages, not this runtime.
  The private terminal and module are part of each attested runtime; the build
  recipe pins upstream kmscon and libtsm, verifies libseat linkage and emits the
  digest-bound capability record used by sessiond. Its explicit DRM patch accepts
  an already-master logind descriptor without privileged drmSetMaster. Distro kmscon is never used as
  an implicit fallback.
* After server-side authorization eligibility checks, call
  `linux::prepare_official_release("vX.Y.Z")`. Download uses a fixed official
  GitHub release URL with an unprivileged `nobody` child, no credentials or inherited
  environment, HTTPS-only redirects, 120-second deadline and 512 MiB size limit.
  GitHub may redirect to its HTTPS asset CDN; all returned bytes remain untrusted
  until cryptographic verification. Never pass arbitrary URLs or paths from D-Bus.
* Verification binds repository, certificate identity, workflow, master ref,
  source/signing SHA, platform, protocol, version and runtime SHA-256. The signed
  runtime contains a second copy of the manifest (its self-digest is 64 zeroes),
  preventing substitution of the envelope's version or other metadata.
* Present the verified release metadata in the trusted confirmation interface.
  After confirmation, sessiond must block new logins, complete PAM logout and write
  root-owned `/run/tundra/maintenance-ready` containing `sessions-closed`. Dispatch
  the independent `tundra-update@vX.Y.Z.service`; do not execute apply inside a
  service that it will restart. The template uses `apply-prepared` with a validated
  release ID, never a shell command string.
* `apply_prepared` saves the previous version durably before stopping services and
  switching the atomic symlink, starts sessiond/privileged and checks their active
  status. Failure restores the recorded previous version. At boot run `recover`
  before sessiond/privileged: an uncommitted transaction restores the previous
  runtime even if power failed after the symlink was switched. Recovery also clears
  the protected maintenance marker when a crash occurred after logout but before
  the transaction journal was created. It never starts services itself.

The package must integrate the maintenance-ready handshake with sessiond so no
new login is accepted between logout and update completion. A root marker alone
is not a substitute for that lifecycle state. Service liveness is the automated
health gate; physical greeter/input rendering is also part of release acceptance.
The release workflow is manually dispatched on reviewed `master`; preparing it
does not publish anything. Release asset replacement is intentionally refused.

## Offline migration

Example (the ordinary UID comes from NSS; do not substitute an application user ID):

```sh
sudo /usr/libexec/tundra/tundra-system-maintenance migrate-legacy \
  --source /root/tundra-legacy-export --uid 1001
sudo /usr/libexec/tundra/tundra-system-maintenance migrate-legacy \
  --source /root/tundra-legacy-export --uid 1001 --apply
```

The default is dry-run. The explicit source directory may contain `config.toml`
and a `content/` directory. It must be root-owned, have root-owned non-writable
ancestors and contain no symlinks or special files. Copy only the intended legacy
config/content into that reviewed source; the tool does not scan `/root` or import
an entire old profile. The target must have no active logind sessions.

Only typed appearance, language, timezone, weather, explorer and editor preferences
are imported into `$HOME/.config/TundraUX3/config.toml`; new configuration defaults
supply security/launcher settings. Content goes into
`$HOME/Documents/Tundra-import/`. The target uses NSS HOME and XDG defaults, never
root's inherited XDG variables. Credentials, roles, sessions, history, command
shortcuts and old authorization are excluded. Existing destinations are skipped,
the source is preserved, and nothing recursively chowns HOME. Actual writes run
in a permanently unprivileged target-UID child using directory file descriptors
and `O_NOFOLLOW|O_EXCL`, avoiding symlink races and overwrite. A failure is reported;
already imported files remain and a retry skips them.
