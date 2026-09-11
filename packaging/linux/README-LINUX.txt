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
system D-Bus, a working DRM device, libseat with the logind backend and a CJK font.
The initial backend supports seat0. Packages build and carry a PRIVATE kmscon
from pinned upstream commit ad9c77bc04f718d0f0d6dfc51291b7d652336429 (v10.0.3),
with libseat explicitly enabled. The old distro executable is never a fallback.

Version or --libseat appearing in --help does not prove backend support: the
Fedora 43 kmscon 10.0.3 package accepted that option without a compiled libseat
backend and fell back to direct input access. Tundra checks the private binary's
root-owned capability record and digest, including its Pango font module. The
build recipe verifies actual DT_NEEDED linkage to libseat; physical device handoff
still requires native testing. Do not repair failures using raw input ACLs.

The build pins libtsm 4.7.0 and embeds it using a small build-description change
from shared_library to library with default_library=static. This removes the old
Ubuntu system libtsm ABI constraint. A second exact source patch recognizes an
already-master DRM descriptor supplied by logind before calling drmSetMaster;
without it the ordinary UID fails with EPERM despite holding the brokered device.
The capability digest binds the resulting patched binary. Pango is shipped at
/usr/libexec/tundra/modules/kmscon/mod-pango.so, resolving into the active runtime.
Both upstream licenses are included. Run packaging/linux/build-kmscon.sh only in
the trusted build environment; it never installs files onto the host.

Fedora 43: the RPM requires libseat, systemd, PAM, D-Bus, curl, Pango, Noto CJK fonts
and gh >= 2.87.3. Ubuntu 24.04: the deb uses its libseat/DRM/Pango libraries with
the private terminal, without depending on its old kmscon package. System updates
need a gh CLI supporting --source-ref, --source-digest, --signer-digest and
--custom-trusted-root; a qualified gh version is recommended separately.

SELinux must remain Enforcing. Do not relabel broad filesystem trees, disable
SELinux, or use permissive mode to conceal a denied operation. A successful build
or package install is not evidence that independent sessions or input isolation
work; verify those on the actual seat and verify versioned executable SELinux
labels before enabling services.

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
versions/vVERSION/{bin,share,release.json}, including the private kmscon, its
digest-bound capability record and Pango module; post-install creates current only if
absent. The current pointer and downloaded versions are runtime state, not
package-owned files, so an ordinary package upgrade does not overwrite a newer
online runtime. If an OS package upgrade removed the previous bootstrap version,
post-install repairs only that dangling version pointer to the new bootstrap.
The legacy public /usr/share/tundraux3/assets stays a directory with packaged
defaults; local untracked custom files are not deleted to replace it with a link.
Versioned executables resolve their own version resources before this fallback.
Installing policies reloads the live system D-Bus configuration without restarting
the bus. Schedule OS package upgrades outside active desktop sessions. The maintenance
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
TUNDRAUX3_KMSCON_BUILD_DIR can explicitly reuse the complete vetted private-terminal
build output; staging verifies binary/module hashes and the pinned source commit.
`scripts/stage-linux-system.py` only writes an empty, explicitly selected staging
directory and accepts reviewed trust roots; it never installs on the host.
