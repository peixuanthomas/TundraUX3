#!/usr/bin/env python3
"""Inspect production RPM/DEB packaging with prebuilt native debug payloads.

Run only in a new disposable Fedora container. Supply the source subset and
Shell/CLI at /srv/tundra-package-fixture as documented in docs/scripts/tests/README.md.
This does not claim to run the release-profile Cargo build or a Debian runtime.
"""
from pathlib import Path
import json, os, shutil, subprocess, tarfile
assert os.getuid() == 0 and Path('/run/.containerenv').exists()
root = Path('/srv/tundra-package-fixture')
source = root / 'source'
work = root / 'work'
work.mkdir(exist_ok=False)
version = '1.3.1'
name = f'tundraux3-{version}-linux-x86_64'
stage = work / name
stage.mkdir()
for binary in ('tundra-shell', 'tundra-cli'):
    shutil.copy2(root / binary, stage / binary)
    subprocess.run(['strip', str(stage / binary)], check=True)
shutil.copytree(source / 'crates/ascii-assets/assets', stage / 'assets')
for origin, dest in [('LICENSE', 'LICENSE'), ('crates/weathr/LICENSE.weathr', 'LICENSE.weathr'), ('docs/packaging/linux/README-LINUX.txt', 'README-LINUX.txt'), ('packaging/linux/tundra-installation.json', 'tundra-installation.json')]:
    shutil.copy2(source / origin, stage / dest)
assert json.loads((stage / 'tundra-installation.json').read_text()) == {'format': 1, 'kind': 'portable-user'}
archive = work / f'{name}.tar.gz'
with tarfile.open(archive, 'w:gz') as out:
    out.add(stage, arcname=name)
rpmroot = work / 'rpm'
for folder in ('BUILD', 'BUILDROOT', 'RPMS', 'SOURCES', 'SPECS', 'SRPMS'):
    (rpmroot / folder).mkdir(parents=True)
shutil.copy2(archive, rpmroot / 'SOURCES' / archive.name)
shutil.copy2(source / 'packaging/debian/tundraux3.desktop', rpmroot / 'SOURCES/tundraux3.desktop')
subprocess.run(['rpmbuild', '-bb', '--define', f'_topdir {rpmroot}', '--define', f'tundra_version {version}', str(source / 'packaging/rpm/tundraux3.spec')], check=True)
rpm = next((rpmroot / 'RPMS/x86_64').glob('*.rpm'))

def output(*args):
    return subprocess.check_output(args, text=True)
requires = output('rpm', '-qp', '--requires', str(rpm))
files = output('rpm', '-qpl', str(rpm))
scripts = output('rpm', '-qp', '--scripts', str(rpm))
assert 'PackageKit\n' in requires and 'polkit\n' in requires
assert 'sudo' not in requires and 'pam' not in requires.lower()
assert not scripts.strip()
for forbidden in ('/pam.d/', 'tundra-installation.json', '/systemd/', '/sysusers.d/', '/tmpfiles.d/'):
    assert forbidden not in files
subprocess.run(['dnf', '-y', 'install', str(rpm)], check=True)
assert output('rpm', '-qf', '/usr/bin/tundra-shell').startswith('tundraux3-1.3.1-1')
assert output('rpm', '-qf', '/usr/bin/pkttyagent').startswith('polkit-')
assert subprocess.run(['/usr/bin/tundra-shell'], stdout=subprocess.PIPE, stderr=subprocess.STDOUT).returncode != 0
print('RPM: production spec, dependencies, installed ownership, pkttyagent provider and root guard PASS', flush=True)
deb = work / 'deb'
for binary in ('tundra-shell', 'tundra-cli'):
    (deb / 'usr/bin').mkdir(parents=True, exist_ok=True)
    shutil.copy2(stage / binary, deb / 'usr/bin' / binary)
shutil.copytree(stage / 'assets', deb / 'usr/share/tundraux3/assets')
(deb / 'usr/share/applications').mkdir(parents=True)
shutil.copy2(source / 'packaging/debian/tundraux3.desktop', deb / 'usr/share/applications/tundraux3.desktop')
(deb / 'DEBIAN').mkdir()
(deb / 'DEBIAN/control').write_text((source / 'packaging/debian/control').read_text().replace('@VERSION@', version))
package = work / 'tundraux3_1.3.1_amd64.deb'
subprocess.run(['dpkg-deb', '--build', '--root-owner-group', str(deb), str(package)], check=True)
control = output('dpkg-deb', '--field', str(package))
assert 'sudo' not in control and 'libpam' not in control
files = output('dpkg-deb', '--contents', str(package))
for forbidden in ('/pam.d/', 'tundra-installation.json', '/systemd/', '/sysusers.d/', '/tmpfiles.d/'):
    assert forbidden not in files
print('DEB: production control and package payload inspection PASS (not a Debian runtime test)', flush=True)
print('Portable: formal marker and ordinary executable/assets payload PASS', flush=True)
print('Validation uses stripped debug payloads; this is not a release-profile build.', flush=True)
