#!/usr/bin/env python3
"""Regression tests for locale-independent localization source decoding."""
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
from io import StringIO
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "check-localization.py"
SPEC = importlib.util.spec_from_file_location("check_localization", SCRIPT)
checker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(checker)
READ_TEXT = Path.read_text


def read_text_with_windows_default(path, encoding=None, errors=None):
    """Reproduce a non-UTF-8 Windows default on every test host."""
    return READ_TEXT(path, encoding=encoding or "cp1252", errors=errors)


class CheckLocalizationTests(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.locales = self.root / "crates/ascii-assets/assets/locales"
        self.source = self.root / "crates/example/src/main.rs"
        self.source.parent.mkdir(parents=True)
        for code in ("en-US", "zh-CN"):
            (self.locales / code).mkdir(parents=True)
        self.enterContext(patch.multiple(checker, ROOT=self.root, LOCALES=self.locales))
        self.enterContext(patch.object(Path, "read_text", read_text_with_windows_default))

    def write_fixture(self, english="hello = Hello\n", chinese="hello = Hello\n",
                      source='i18n::msg!("hello");\n'):
        for code, content in (("en-US", english), ("zh-CN", chinese)):
            (self.locales / code / "messages.ftl").write_text(content, encoding="utf-8")
        self.source.write_text(source, encoding="utf-8")

    def run_checker(self):
        stdout, stderr = StringIO(), StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            result = checker.main()
        return result, stdout.getvalue(), stderr.getvalue()

    def test_utf8_fluent_with_non_utf8_default(self):
        for code in ("en-US", "zh-CN"):
            with self.subTest(code=code):
                # UTF-8 for this text includes byte 0x81, undefined in cp1252.
                content = "hello = 消息\n"
                self.write_fixture(**{"english" if code == "en-US" else "chinese": content})
                with self.assertRaises(UnicodeDecodeError):
                    (self.locales / code / "messages.ftl").read_text()
                self.assertEqual(self.run_checker(), (
                    0, "Validated 1 literal call sites and 1 paired en-US/zh-CN messages.\n", ""))

    def test_utf8_rust_with_non_utf8_default(self):
        self.write_fixture(source='// 消息\ni18n::msg!("hello");\n')
        with self.assertRaises(UnicodeDecodeError):
            self.source.read_text()
        self.assertEqual(self.run_checker(), (
            0, "Validated 1 literal call sites and 1 paired en-US/zh-CN messages.\n", ""))

    def test_missing_translation_still_fails(self):
        self.write_fixture(english="hello = 消息\nmissing = Missing\n", chinese="hello = 消息\n")
        self.assertEqual(self.run_checker(), (
            1, "", "zh-CN: missing bundled translation missing\n"))

    def test_unknown_call_still_reports_line(self):
        self.write_fixture(source='// 消息\ni18n::msg!("missing");\n')
        expected = f"{self.source.relative_to(self.root)}:2: unknown message missing\n"
        self.assertEqual(self.run_checker(), (1, "", expected))


if __name__ == "__main__":
    unittest.main()
