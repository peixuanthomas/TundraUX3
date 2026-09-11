"""Structural packaging checks; fixture binaries/roots are deliberately not executable releases."""
import importlib.util
import json
import hashlib
from pathlib import Path
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'stage-linux-system.py'
spec = importlib.util.spec_from_file_location('stage_linux_system', SCRIPT)
staging = importlib.util.module_from_spec(spec)
spec.loader.exec_module(staging)


class StageLinuxSystemTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='tundra-package-test-')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.binaries = self.base / 'bin'
        self.binaries.mkdir()
        for name in (*staging.BINARIES, 'tundra-system-maintenance'):
            (self.binaries / name).write_bytes(b'not-an-executable: packaging layout fixture\n')
        self.kmscon = self.base / 'kmscon-build'
        self.kmscon.mkdir()
        for name in ('kmscon', 'mod-pango.so', 'LICENSE.kmscon', 'LICENSE.libtsm'):
            (self.kmscon / name).write_bytes(b'private-terminal-packaging-fixture')
        digest = hashlib.sha256(b'private-terminal-packaging-fixture').hexdigest()
        (self.kmscon / 'kmscon-capabilities.json').write_text(json.dumps(dict(sha256=digest, pango_sha256=digest, libseat=True, source_commit=staging.KMSCON_COMMIT)))
        self.roots = self.base / 'roots.jsonl'
        self.roots.write_text(json.dumps({'mediaType': 'test-fixture-only', 'certificateAuthorities': [{}]}) + '\n')

    def stage(self, flavor='deb'):
        root = self.base / flavor
        staging.stage(root, self.binaries, '1.3.0', 'a' * 40, self.roots, flavor, self.kmscon)
        return root

    def test_versioned_runtime_and_stable_entrypoints(self):
        root = self.stage()
        version = root / staging.RUNTIME / 'versions/v1.3.0'
        for name in staging.BINARIES:
            self.assertEqual((version / 'bin' / name).stat().st_mode & 0o777, 0o755)
        self.assertEqual((root / 'usr/bin/tundra-shell').readlink(), Path('/var/lib/tundra/runtime/current/bin/tundra-shell'))
        self.assertFalse((root / 'usr/libexec/tundra/tundra-system-maintenance').is_symlink())
        self.assertEqual((root / 'usr/libexec/tundra/kmscon').readlink(),Path('/var/lib/tundra/runtime/current/bin/kmscon'))
        self.assertEqual((root / 'usr/libexec/tundra/modules/kmscon').readlink(),Path('/var/lib/tundra/runtime/current/share/tundra/kmscon-modules'))
        self.assertTrue((version / 'bin/kmscon-capabilities.json').is_file())
        self.assertTrue((version / 'share/tundra/licenses/LICENSE.kmscon').is_file())
        self.assertFalse((root / staging.RUNTIME / 'current').exists(), 'current must remain post-install runtime state')
        self.assertEqual((root / 'usr/share/tundra/bootstrap-release').read_text().strip(), 'v1.3.0')
        for locale in ('en-US', 'zh-CN'):
            self.assertTrue((version / f'share/tundraux3/assets/locales/{locale}/manifest.toml').is_file())
            self.assertTrue((version / f'share/tundra/greeter/locales/{locale}/greeter.ftl').is_file())
        self.assertTrue((root / 'etc/tundra/update-trusted-root.jsonl').is_file())
        self.assertFalse((root / 'etc/pam.d/tundraux3').exists())
        self.assertTrue((root / 'usr/lib/tmpfiles.d/tundra.conf').is_file())
        self.assertFalse((root / 'etc/systemd/system/multi-user.target.wants').exists())

    def test_distribution_pam_policies(self):
        deb = self.stage('deb')
        rpm = self.stage('rpm')
        self.assertIn('@include common-session', (deb / 'etc/pam.d/tundra-session').read_text())
        self.assertIn('system-auth', (rpm / 'etc/pam.d/tundra-session').read_text())
        for root in (deb, rpm):
            self.assertTrue((root / 'usr/share/dbus-1/system.d/org.tundra.Session1.conf').is_file())
            self.assertTrue((root / 'usr/share/dbus-1/system.d/org.tundra.Privileged1.conf').is_file())

    def test_refuses_missing_trust_and_existing_destination(self):
        self.roots.write_text('')
        with self.assertRaises(ValueError):
            self.stage()
        self.assertFalse((self.base / 'deb').exists())
        self.roots.write_text(json.dumps({'mediaType': 'test-fixture-only', 'certificateAuthorities': [{}]}))
        root = self.stage()
        with self.assertRaises(ValueError):
            self.stage()
        self.assertTrue((root / 'usr/share/tundra/bootstrap-release').is_file())

    def test_refuses_symlink_binary_and_path_version(self):
        program = self.binaries / 'tundra-shell'
        program.unlink()
        program.symlink_to('/bin/sh')
        with self.assertRaises(ValueError):
            self.stage()
        with self.assertRaises(ValueError):
            staging.stage(self.base / 'invalid', self.binaries, '../../escape', 'a' * 40, self.roots, 'deb', self.kmscon)


    def test_refuses_unbound_private_terminal(self):
        (self.kmscon / 'kmscon').write_bytes(b'replaced executable')
        with self.assertRaises(ValueError):
            self.stage()


if __name__ == '__main__':
    unittest.main()
