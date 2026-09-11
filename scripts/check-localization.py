#!/usr/bin/env python3
"""Check literal production message references against the canonical English pack.

Fluent syntax, references and parameter contracts are validated by i18n's build
and behavioral tests. This check catches misspelled IDs at Rust call sites.
"""
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
LOCALES = ROOT / "crates/ascii-assets/assets/locales"
ENTRY = re.compile(r"^([A-Za-z][A-Za-z0-9_-]*)\s*=", re.MULTILINE)
CALL = re.compile(r'(?:i18n::(?:msg|tr)!\s*\(\s*|localize!\s*\([^,]+,\s*)"([A-Za-z][A-Za-z0-9_-]*)"')


def entries(code):
    result = set()
    for path in sorted((LOCALES / code).rglob("*.ftl")):
        for identifier in ENTRY.findall(path.read_text()):
            if identifier in result:
                raise ValueError(f"{path.relative_to(ROOT)}: duplicate message {identifier}")
            result.add(identifier)
    return result


def main():
    english = entries("en-US")
    chinese = entries("zh-CN")
    errors = []
    for identifier in sorted(english - chinese):
        errors.append(f"zh-CN: missing bundled translation {identifier}")
    for identifier in sorted(chinese - english):
        errors.append(f"zh-CN: unknown bundled message {identifier}")
    count = 0
    for path in sorted((ROOT / "crates").glob("*/src/**/*.rs")):
        if "tests" in path.parts or path.name == "tests.rs":
            continue
        source = path.read_text()
        # Inline test fixtures intentionally reference synthetic resource IDs.
        source = source.split("#[cfg(test)]\nmod tests", 1)[0]
        for match in CALL.finditer(source):
            count += 1
            if match[1] not in english:
                line = source.count("\n", 0, match.start()) + 1
                errors.append(f"{path.relative_to(ROOT)}:{line}: unknown message {match[1]}")
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print(f"Validated {count} literal call sites and {len(english)} paired en-US/zh-CN messages.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
