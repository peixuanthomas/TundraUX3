TundraUX3 on Linux: ordinary user UX and opt-in independent sessions
=================================================================

Run ./tundra-shell or tundra-shell as your ordinary system user. Do not use sudo
for the UX. HOME, XDG paths and credentials belong to the real login account.
Portable source self-updates operate only inside user-owned installations.

The deb/RPM includes opt-in sessiond, trusted greeter, privileged service and
system maintenance. Installation creates tundra-greeter and tundra-admin but
NEVER adds people to the administrator group, starts/enables a seat, replaces
SDDM or another display manager, or changes the default boot target. The
system-root directory in a distribution tarball is package staging data; do not
copy it into / manually. A --tar-only build contains ordinary UX only.

Prerequisites
-------------
Independent seat mode requires systemd/logind, complete PAM session stacks,
system D-Bus, a working DRM device, and kmscon 10.0.3 or newer with libseat
support and a CJK-capable font. The initial backend supports seat0.

Fedora 43: the RPM requires kmscon >= 10.0.3, gh >= 2.87.3, systemd, PAM, D-Bus,
curl and Noto CJK fonts. If an enabled repository does not yet carry the required
kmscon, installation must wait for that qualified dependency; do not force it.
SELinux must remain Enforcing. Do not relabel broad filesystem trees, disable
SELinux, or use permissive mode to conceal a denied operation.

Ubuntu 24.04: the standard archive kmscon is 9.0.0 and is insufficient for this
independent-session backend. The deb therefore recommends newer kmscon/gh while
preserving ordinary UX installation. Independent sessions require a separately
qualified administrator-installed kmscon >= 10.0.3; system updates require gh
supporting --source-ref, --source-digest, --signer-digest and
--custom-trusted-root. An Ubuntu build or package install alone is NOT evidence
that independent session or trusted input isolation works.

Official package references:
https://packages.ubuntu.com/noble/amd64/utils/kmscon
https://packages.fedoraproject.org/pkgs/kmscon/kmscon/

Installation and lifecycle
--------------------------
The PAM policies are /etc/pam.d/tundra-session (login/unlock) and
/etc/pam.d/tundra-greeter (the isolated service account). The old root-UX
/etc/pam.d/tundraux3 policy is no longer installed or used. It may remain as a
locally modified obsolete config when upgrading an old package; remove it only
after reviewing local changes. sudo is not a runtime dependency.

Root policy is /etc/tundra/privileged.toml. An administrator explicitly adds
chosen users to tundra-admin; sudo/wheel membership alone does not grant Tundra
click authorization. Lock/unlock keeps the user's processes and logind session;
logout closes PAM and terminates that session. Switching users logs out first.

Before manually enabling services, keep an SSH recovery connection, verify
`tundra-cli debug doctor`, and arrange a deliberate exclusive-seat test window.
Stop the existing display manager only when intentionally taking over the seat.
The relevant units are tundra-update-recover, tundra-sessiond and
tundra-privileged. Start recovery before the other two. Enable them at boot only
after actual display/input and login/logout tests pass. Recovery must run before
both services at boot and must not be repeatedly invoked during an online update.
To revert a test, stop Tundra services and start the original display manager;
installation itself does not alter that manager's enabled state.

Updates and legacy data
-----------------------
Stable public and libexec symlinks resolve through
/var/lib/tundra/runtime/current/bin. The initial package installs
versions/vVERSION/{bin,share,release.json}; post-install creates current only if
absent. The current pointer and downloaded versions are runtime state, not
package-owned files, so an ordinary package upgrade does not overwrite a newer
online runtime. If an OS package upgrade removed the previous bootstrap version,
post-install repairs only that dangling version pointer to the new bootstrap.
Schedule OS package upgrades outside active desktop sessions. The maintenance
executable remains a regular package-managed
file at /usr/libexec/tundra/tundra-system-maintenance.

Build-time `gh attestation trusted-root` supplies the package's protected
/etc/tundra/update-trusted-root.jsonl. Packaging fails if roots are unavailable;
installation does not download trust. An offline trusted builder can set
TUNDRAUX3_TRUSTED_ROOT to a reviewed file. Runtime signature verification uses
that file, exact official workflow/master/source identity and bounded extraction.
The trusted confirmation happens only after verification; applying closes the
session and switches versions, with durable previous-version rollback.

Explicit offline migration, first inspect then apply:
  sudo tundra-cli migrate-legacy --source /root/reviewed-legacy-export --uid 1001
  sudo tundra-cli migrate-legacy --source /root/reviewed-legacy-export --uid 1001 --apply
The source may contain config.toml and content/. Credentials, roles and sessions
are excluded; existing destination files are skipped. Actual writes run under
the target UID. See docs/linux/system-maintenance.md in the source repository.

Build/testing notes
-------------------
`scripts/package-linux.sh` builds deb+portable; --rpm builds RPM+portable;
--tar-only builds only ordinary UX. All release builds use Cargo.lock.
TUNDRAUX3_PREBUILT_BIN_DIR permits an explicit developer packaging smoke test
without rebuilding; do not use arbitrary prebuilt binaries for a trusted release.
`scripts/stage-linux-system.py` only writes an empty, explicitly selected staging
directory and accepts reviewed trust roots; it never installs on the host.
