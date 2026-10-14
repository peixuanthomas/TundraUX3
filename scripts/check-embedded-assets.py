#!/usr/bin/env python3
"""Check that each default PNG occurs exactly once in each supplied binary."""

import argparse
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--allow-missing", action="store_true",
        help="allow the linker to omit unused PNGs; still reject repeated copies",
    )
    parser.add_argument("binaries", type=Path, nargs="+")
    args = parser.parse_args()
    theme = Path(__file__).resolve().parents[1] / "crates/ascii-assets/assets/themes/default"
    images = {}
    for path in sorted(theme.rglob("*.png")):
        images.setdefault(path.read_bytes(), []).append(str(path.relative_to(theme)))
    if not images:
        parser.error("no default PNGs found")

    failed = False
    for binary in args.binaries:
        contents = binary.read_bytes()
        count = 0
        total = 0
        for image, names in images.items():
            copies = contents.count(image)
            count += copies
            total += copies * len(image)
            if copies > 1 or (copies == 0 and not args.allow_missing):
                failed = True
                expected = "at most 1 copy" if args.allow_missing else "1 copy"
                print(f"FAIL {binary}: {', '.join(names)}: expected {expected}, found {copies}")
        print(f"{binary}: {len(images)} unique PNGs, {count} embedded copies, {total} PNG bytes")
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
