#!/usr/bin/env python3
"""Prepare signed, fixed-name RPM fixtures inside the disposable Fedora container.

Copy the built packagekit-update-probe to /srv/tundra-fixture/probe first.
This script never runs on the host and never accepts a package/command target.
"""
from pathlib import Path
import os
import shutil
import subprocess


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def main():
    if os.geteuid() != 0 or not Path('/run/.containerenv').is_file():
        raise SystemExit('Run only inside the disposable rootless Fedora test container')
    # Fedora service mount isolation cannot be nested in this rootless container.
    # Keep this override confined to the disposable fixture, never RPM packaging.
    override = Path('/etc/systemd/system/polkit.service.d')
    override.mkdir(parents=True, exist_ok=True)
    (override / 'container.conf').write_text("[Service]\n" + "".join(
        f"{setting}=no\n" for setting in (
            'PrivateDevices', 'PrivateNetwork', 'PrivateTmp', 'ProtectHome',
            'ProtectSystem', 'ProtectControlGroups', 'ProtectKernelModules',
            'ProtectKernelLogs', 'ProtectKernelTunables', 'ProtectClock', 'ProtectHostname',
            'LockPersonality', 'MemoryDenyWriteExecute', 'RestrictNamespaces',
            'RestrictRealtime', 'RestrictSUIDSGID'
        )
    ) + 'RestrictAddressFamilies=\nSystemCallArchitectures=\nSystemCallFilter=\n')
    run('systemctl', 'daemon-reload')
    run('systemctl', 'start', 'polkit')
    root = Path('/srv/tundra-fixture')
    probe = root / 'probe'
    if not probe.is_file():
        raise SystemExit('Missing the built native test probe at the fixed fixture path')
    if (root / 'prepared').exists():
        raise SystemExit('Fixture already prepared; use a fresh disposable container to reset it')
    if subprocess.run(['id', '-u', 'tundra-test'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
        run('useradd', '--create-home', '--uid', '1000', 'tundra-test')
    keys = root / 'keys'
    keys.mkdir(mode=0o700, parents=True, exist_ok=True)
    env = dict(os.environ, GNUPGHOME=str(keys))
    public_key = root / 'fixture-key.asc'
    if not public_key.exists():
        run('gpg', '--batch', '--pinentry-mode', 'loopback', '--passphrase', '', '--quick-generate-key',
            'Tundra disposable fixture <fixture@example.invalid>', 'rsa2048', 'sign', '1d', env=env)
        with public_key.open('wb') as output:
            run('gpg', '--batch', '--armor', '--export', 'fixture@example.invalid', env=env, stdout=output)
    listing = subprocess.check_output(['gpg', '--batch', '--with-colons', '--list-keys', 'fixture@example.invalid'], env=env, text=True)
    fingerprint = next(line.split(':')[9] for line in listing.splitlines() if line.startswith('fpr:'))
    run('rpm', '--import', str(public_key))
    build = root / 'build'
    for directory in ['SPECS', 'SOURCES', 'BUILD', 'BUILDROOT', 'RPMS', 'SRPMS']:
        (build / directory).mkdir(parents=True, exist_ok=True)
    shutil.copy2(probe, build / 'SOURCES/probe')
    repo = root / 'repo'
    repo.mkdir(exist_ok=True)
    initial = []
    for name, versions in [('tundraux3', ['1.0.0', '2.0.0']), ('tundra-runtime', ['1.0.0']), ('tundra-unrelated', ['1.0.0', '2.0.0'])]:
        for version in versions:
            binary = name == 'tundraux3'
            payload = '/opt/tundra-fixture/bin/packagekit-update-probe' if binary else f'/opt/tundra-fixture/{name}.txt'
            requires = 'Requires: tundra-runtime >= 1.0.0\n' if binary and version == '2.0.0' else ''
            install = f'install -Dm755 %{{SOURCE0}} %{{buildroot}}{payload}' if binary else f'install -Dm644 /dev/null %{{buildroot}}{payload}'
            spec = build / 'SPECS' / f'{name}-{version}.spec'
            spec.write_text(f'''%global debug_package %{{nil}}
%global __os_install_post %{{nil}}
Name: {name}
Version: {version}
Release: 1
Summary: Disposable Tundra update integration fixture
License: MIT
BuildArch: {'x86_64' if binary else 'noarch'}
Source0: probe
{requires}
%description
Disposable test payload for the fixed-package PackageKit integration.
%prep
%build
%install
{install}
%files
{payload}
''')
            run('rpmbuild', '-bb', '--define', f'_topdir {build}', str(spec))
            packages = list((build / 'RPMS').glob(f'*/{name}-{version}-1.*.rpm'))
            if len(packages) != 1:
                raise RuntimeError(f'Expected one RPM for {name} {version}')
            package = packages[0]
            run('rpmsign', '--define', f'_openpgp_sign_id {fingerprint}', '--define', '_openpgp_sign gpg', '--addsign', str(package), env=env)
            run('rpmkeys', '--checksig', str(package))
            if name in ('tundraux3', 'tundra-unrelated') and version == '1.0.0':
                initial.append(str(package))
            else:
                shutil.copy2(package, repo / package.name)
    run('rpm', '--upgrade', *initial)
    run('createrepo_c', str(repo))
    backup = root / 'original-repositories'
    backup.mkdir(exist_ok=True)
    for source in Path('/etc/yum.repos.d').glob('*.repo'):
        shutil.move(str(source), backup / source.name)
    Path('/etc/yum.repos.d/tundra-fixture.repo').write_text(f'''[test-updates]
name=Signed disposable Tundra fixture
baseurl=file://{repo}
enabled=1
gpgcheck=1
repo_gpgcheck=0
gpgkey=file://{public_key}
metadata_expire=0
''')
    # Backend transaction tests use an explicit policy for this fixture user only.
    # Agent tests remove this rule and exercise the actual polkit authentication flow.
    Path('/etc/polkit-1/rules.d/49-tundra-fixture.rules').write_text('''polkit.addRule(function(action, subject) {
    if (action.id == "org.freedesktop.packagekit.system-update" && subject.user == "tundra-test") {
        return polkit.Result.YES;
    }
});
''')
    (root / 'prepared').write_text(f'public-signing-key={fingerprint}\n')
    print('Signed fixture prepared; frontend tests must run as tundra-test (UID 1000).')


if __name__ == '__main__':
    main()
