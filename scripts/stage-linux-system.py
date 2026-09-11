#!/usr/bin/env python3
"""Stage an opt-in Linux system installation; never alter the host's root filesystem."""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import stat

REPO = Path(__file__).resolve().parents[1]
RUNTIME = Path('var/lib/tundra/runtime')
BINARIES = ('tundra-shell', 'tundra-cli', 'tundra-sessiond', 'tundra-greeter', 'tundra-privileged')
KMSCON_COMMIT = 'ad9c77bc04f718d0f0d6dfc51291b7d652336429'


def regular(source):
    if not source.is_file() or source.is_symlink():
        raise ValueError(f'Expected regular source file: {source}')


def copy(source, target, mode=0o644):
    regular(source)
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    target.chmod(mode)


def tree(source, target):
    for item in sorted(source.rglob('*')):
        if item.is_symlink():
            raise ValueError(f'Symlink in packaged resources: {item}')
        if item.is_file():
            copy(item, target / item.relative_to(source))
        elif not item.is_dir():
            raise ValueError(f'Special file in packaged resources: {item}')


def validate_roots(path):
    regular(path)
    if path.stat().st_size > 4 * 1024 * 1024:
        raise ValueError('Trusted root file exceeds size bound')
    roots = [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
    if not roots or any(not isinstance(root, dict) or not root.get('mediaType') or
                        not root.get('certificateAuthorities') for root in roots):
        raise ValueError('Expected nonempty gh attestation trusted-root JSONL output')


def stage(root, binaries, version, source_sha, trusted_root, flavor, kmscon):
    if not re.fullmatch(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)', version):
        raise ValueError('Version must be canonical MAJOR.MINOR.PATCH')
    if not re.fullmatch(r'[0-9a-f]{40}', source_sha):
        raise ValueError('Expected exact source commit SHA')
    validate_roots(trusted_root)
    import hashlib
    for name in ('kmscon', 'mod-pango.so', 'kmscon-capabilities.json', 'LICENSE.kmscon', 'LICENSE.libtsm'):
        regular(kmscon / name)
    capabilities = json.loads((kmscon / 'kmscon-capabilities.json').read_text())
    if capabilities.get('libseat') is not True or capabilities.get('source_commit') != KMSCON_COMMIT:
        raise ValueError('Private kmscon does not identify the vetted libseat-enabled build')
    if capabilities.get('sha256') != hashlib.sha256((kmscon / 'kmscon').read_bytes()).hexdigest() or capabilities.get('pango_sha256') != hashlib.sha256((kmscon / 'mod-pango.so').read_bytes()).hexdigest():
        raise ValueError('Private terminal or font module digest mismatch')
    if root.is_symlink():
        raise ValueError('Staging destination must not be a symlink')
    root = root.resolve()
    if root in (Path('/'), REPO) or (root.exists() and any(root.iterdir())):
        raise ValueError('Staging destination must be a new or empty dedicated directory')
    for name in (*BINARIES, 'tundra-system-maintenance'):
        regular(binaries / name)
    root.mkdir(parents=True, exist_ok=True)
    release = f'v{version}'
    installed = root / RUNTIME / 'versions' / release
    for name in BINARIES:
        copy(binaries / name, installed / 'bin' / name, 0o755)
    copy(kmscon / 'kmscon', installed / 'bin/kmscon', 0o755)
    copy(kmscon / 'kmscon-capabilities.json', installed / 'bin/kmscon-capabilities.json')
    copy(kmscon / 'mod-pango.so', installed / 'share/tundra/kmscon-modules/mod-pango.so')
    for license_name in ('LICENSE.kmscon', 'LICENSE.libtsm'):
        copy(kmscon / license_name, installed / 'share/tundra/licenses' / license_name)
    assets = REPO / 'crates/ascii-assets/assets'
    tree(assets, installed / 'share/tundraux3/assets')
    for locale in ('en-US', 'zh-CN'):
        copy(assets / 'locales' / locale / 'greeter.ftl', installed / 'share/tundra/greeter/locales' / locale / 'greeter.ftl')
    manifest = dict(version=release, source_sha=source_sha,
                    architecture='x86_64-unknown-linux-gnu', protocol=1,
                    runtime_sha256='0' * 64)
    # Initial runtime trust comes from the OS package signature, not online attestation.
    (installed / 'release.json').write_text(json.dumps(manifest) + '\n')
    (installed / 'share/tundra/release.json').write_text(json.dumps(manifest) + '\n')
    copy(binaries / 'tundra-system-maintenance', root / 'usr/libexec/tundra/tundra-system-maintenance', 0o755)
    for name in (*BINARIES, 'kmscon'):
        location = Path('usr/bin') if name in ('tundra-shell', 'tundra-cli') else Path('usr/libexec/tundra')
        link = root / location / name
        link.parent.mkdir(parents=True, exist_ok=True)
        link.symlink_to('/' + str(RUNTIME / 'current/bin' / name))
    modules = root / 'usr/libexec/tundra/modules/kmscon'
    modules.parent.mkdir(parents=True, exist_ok=True)
    modules.symlink_to('/' + str(RUNTIME / 'current/share/tundra/kmscon-modules'))
    for name, relative in [('assets', 'usr/share/tundraux3/assets'), ('locales', 'usr/share/tundra/greeter/locales')]:
        link = root / relative
        link.parent.mkdir(parents=True, exist_ok=True)
        target = 'share/tundraux3/assets' if name == 'assets' else 'share/tundra/greeter/locales'
        link.symlink_to('/' + str(RUNTIME / 'current' / target))
    for unit in (REPO / 'packaging/linux/systemd').glob('*.service'):
        copy(unit, root / 'usr/lib/systemd/system' / unit.name)
    for policy in (REPO / 'packaging/linux/dbus-1').rglob('*.conf'):
        copy(policy, root / 'usr/share/dbus-1/system.d' / policy.name)
    if not (root / 'usr/share/dbus-1/system.d/org.tundra.Privileged1.conf').is_file():
        raise ValueError('Privileged service D-Bus policy is missing')
    pam = REPO / 'packaging/linux/pam.d'
    copy(pam / ('tundra-session.fedora' if flavor == 'rpm' else 'tundra-session'), root / 'etc/pam.d/tundra-session')
    copy(pam / 'tundra-greeter', root / 'etc/pam.d/tundra-greeter')
    copy(REPO / 'packaging/linux/privileged.toml', root / 'etc/tundra/privileged.toml')
    copy(trusted_root, root / 'etc/tundra/update-trusted-root.jsonl')
    copy(REPO / 'packaging/linux/sysusers.d/tundra.conf', root / 'usr/lib/sysusers.d/tundra.conf')
    copy(REPO / 'packaging/linux/tmpfiles.d/tundra.conf', root / 'usr/lib/tmpfiles.d/tundra.conf')
    copy(REPO / 'packaging/debian/tundraux3.desktop', root / 'usr/share/applications/tundraux3.desktop')
    for source, name in [('LICENSE', 'copyright'), ('crates/weathr/LICENSE.weathr', 'LICENSE.weathr'), ('packaging/linux/README-LINUX.txt', 'README-LINUX.txt')]:
        copy(REPO / source, root / 'usr/share/doc/tundraux3' / name)
    bootstrap = root / 'usr/share/tundra/bootstrap-release'
    bootstrap.parent.mkdir(parents=True, exist_ok=True)
    bootstrap.write_text(release + '\n')
    # current is runtime state, deliberately not package-owned. postinst creates it
    # only on first install, so package upgrades cannot reset a newer online runtime.
    for directory, dirs, files in os.walk(root, followlinks=False):
        Path(directory).chmod(0o755)
        for name in files:
            path = Path(directory) / name
            if not path.is_symlink() and not (path.stat().st_mode & stat.S_IXUSR):
                path.chmod(0o644)
    return root


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--trusted-root', type=Path, required=True)
    parser.add_argument('--kmscon', type=Path, required=True, help='Output of packaging/linux/build-kmscon.sh')
    parser.add_argument('--flavor', choices=('deb', 'rpm'), required=True)
    args = parser.parse_args()
    stage(args.root, args.binaries, args.version, args.source_sha, args.trusted_root, args.flavor, args.kmscon)


if __name__ == '__main__':
    main()
