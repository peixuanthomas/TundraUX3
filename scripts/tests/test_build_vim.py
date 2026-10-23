#!/usr/bin/env python3
"""Check bundled Vim verification against upstream feature names."""
import importlib.util
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import zipfile
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "build-vim.py"
SPEC = importlib.util.spec_from_file_location("build_vim", SCRIPT)
builder = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(builder)


class VerifyBundleTests(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.bundle = Path(temporary.name)
        for relative in ("runtime/defaults.vim", "runtime/doc/help.txt", "LICENSE"):
            path = self.bundle / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("test\n", encoding="utf-8")
        if builder.os.name == "nt":
            for relative in builder.WINDOWS_ICONV_FILES:
                path = self.bundle / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text("test", encoding="utf-8")
        (self.bundle / "UPSTREAM.json").write_text(json.dumps(builder.PIN), encoding="utf-8")

    def output(self, multi_byte):
        major, minor, patchlevel = builder.PIN["version"].split(".")
        return (f"VIM - Vi IMproved {major}.{minor} (test build)\n"
                f"Included patches: 1-{patchlevel}\n"
                f"+eval +channel +autocmd +timers {multi_byte}\n")

    def test_upstream_multibyte_feature_names(self):
        for feature in ("+multi_byte", "+multi_byte_ime", "+multi_byte_ime/dyn"):
            with self.subTest(feature=feature), patch.object(
                builder.subprocess, "check_output", return_value=self.output(feature)
            ), patch.object(builder, "run"):
                builder.verify_bundle(self.bundle)

    @unittest.skipUnless(builder.os.name == "nt", "Windows bundle dependency")
    def test_missing_windows_encoding_library_is_rejected(self):
        (self.bundle / "libiconv-2.dll").unlink()
        with patch.object(builder.subprocess, "check_output", return_value=self.output("+multi_byte")):
            with self.assertRaisesRegex(RuntimeError, "missing libiconv-2.dll"):
                builder.verify_bundle(self.bundle)

    def test_missing_multibyte_support_is_rejected(self):
        with patch.object(builder.subprocess, "check_output", return_value=self.output("-multi_byte")):
            with self.assertRaisesRegex(RuntimeError, r"requires \+multi_byte"):
                builder.verify_bundle(self.bundle)

    def test_package_contains_runtime_license_and_metadata(self):
        archive = self.bundle / "engine.zip"
        source = self.bundle / "vim"
        source.mkdir()
        for name in ("vim", "LICENSE", "UPSTREAM.json", "runtime/doc/help.txt"):
            path = source / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("test", encoding="utf-8")
        builder.package_bundle(source, archive)
        self.assertEqual(archive.with_suffix(".zip.sha256").read_text(encoding="utf-8"),
                         builder.hashlib.sha256(archive.read_bytes()).hexdigest() + "  engine.zip\n")
        with zipfile.ZipFile(archive) as bundle:
            self.assertEqual(set(bundle.namelist()), {
                "vim/vim", "vim/LICENSE", "vim/UPSTREAM.json", "vim/runtime/doc/help.txt"})
        with self.assertRaisesRegex(ValueError, "outside"):
            builder.package_bundle(source, source / "engine.zip")


if __name__ == "__main__":
    unittest.main()
