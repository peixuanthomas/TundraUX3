#!/usr/bin/env python3
"""Run editor time/heap measurements in fresh processes; keep RSS on stderr.

cargo build --release -p app --example editor_scalability
python3 scripts/benchmark-editor.py > /tmp/editor.csv 2> /tmp/editor-rss.csv
"""
import argparse
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, default=Path('target/release/examples/editor_scalability'))
parser.add_argument('--sizes', nargs='+', type=int, default=[1, 10, 100])
parser.add_argument('--fixtures', nargs='+', default=['ascii-single', 'ascii-lines', 'chinese', 'emoji', 'crlf'])
args = parser.parse_args()
print('fixture,mib,operation,request_ms,total_ms,extra_peak_heap_mib,peak_heap_mib', flush=True)
for fixture in args.fixtures:
    for size in args.sizes:
        subprocess.run([str(args.binary), fixture, str(size)], check=True, timeout=900)
