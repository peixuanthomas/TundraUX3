# Large-file editor support

The editor runs large-file calculations for highlighting, soft wrapping, search, and line-change markers in the background and caches results by viewport or data block, avoiding repeated full-document scans in the main event loop.

## Computation and caching

- **Syntax highlighting:** the viewport retains tokens only for the required logical lines. Parser state is saved every 128 lines, up to 2,048 checkpoints. Edits reuse the unaffected prefix; the first jump to the end of a file parses the prefix in the background to recover multiline comment and string state.
- **Soft wrapping:** cumulative display-line counts for logical lines are built in the background, and display lines are generated on demand. ASCII positions can be calculated directly; long Unicode lines save a byte checkpoint every 256 display lines. Width changes rebuild the index.
- **Search:** plain queries scan the Rope in 64 KiB windows, retaining context for query length and Unicode word boundaries. Interactive search caches at most 512 match positions while continuing to accumulate the full count; F3 reuses results within a page.
- **Replacement:** previews use the same chunked engine. Before applying changes, revision, document generation, original text, and grapheme boundaries are checked; the whole replacement batch is one undo operation. Full previews retain at most 10,000 matches or 8 MiB of data.
- **Background tasks:** files over 64 KiB use background threads. Each feature keeps at most one running task and one pending task; new requests cancel old ones, and stale results are not written back.
- **Line-change markers:** background comparison first skips unchanged prefixes and suffixes. Incremental word counting for a single extremely long ASCII line checks only fixed context on either side of the edit.

## Reproducing benchmarks

```sh
cargo build --release -p app --example editor_scalability
python3 scripts/benchmark-editor.py > editor.csv 2> editor-rss.csv
python3 scripts/benchmark-editor.py --sizes 1 10 --fixtures dense > dense.csv 2> dense-rss.csv
```

The default matrix includes 1, 10, and 100 MiB files with extremely long single ASCII lines, ASCII logs, Chinese, Emoji, and CRLF logs. Each scenario runs in a separate process and measures first-screen highlighting, end-of-file highlighting, soft wrapping, search, and reflow and line markers after editing, in that order.

Metric definitions:

- `request_ms`: main-thread time before the call returns.
- `total_ms`: total time, including background completion and the 1ms polling interval.
- `extra_peak_heap_mib`: peak increase over allocations already live at the start of the operation.
- Linux `VmHWM`: peak RSS of the entire process, including the opened file.

## Reference results

2026-10-13, Linux x86_64, AMD EPYC 9V74, Rust 1.99.0, release build. The observations below are from a single run, intended for implementation comparisons, not as CI timing thresholds.

| Extremely long single ASCII line | Search request (ms) | Search completion (ms) | Extra peak search heap (MiB) |
| --- | ---: | ---: | ---: |
| 1 MiB | 0.137 | 2.288 | 0.124 |
| 10 MiB | 0.179 | 4.463 | 0.128 |
| 100 MiB | 0.135 | 25.403 | 0.128 |

| 100 MiB scenario | Wrap request (ms) | Wrap completion (ms) | Edit and reflow request (ms) | Edit and reflow completion (ms) | Peak RSS (MiB) |
| --- | ---: | ---: | ---: | ---: | ---: |
| Extremely long single ASCII line | 0.018 | 3.235 | 3.067 | 6.775 | 216.5 |
| ASCII logs | 0.048 | 860.124 | 0.434 | 398.413 | 277.8 |
| Chinese | 0.039 | 3784.134 | 0.361 | 254.921 | 217.0 |
| Emoji | 0.059 | 3591.546 | 0.345 | 312.699 | 263.6 |
| CRLF | 0.042 | 755.571 | 0.340 | 311.711 | 274.0 |

The 10 MiB dense scenario contains 5,242,880 matches, with 512 positions retained in the interactive cache. Extra peak search heap is 0.167 MiB, full counting takes 1.39 seconds, and the main-thread request takes 0.110ms.

Raw data: [full matrix](benchmarks/editor-scalability.csv), [peak RSS](benchmarks/editor-scalability-rss.csv), and [dense matches](benchmarks/editor-dense.csv).

## Implementation limits

- Regular expressions retain cross-line, anchor, and capture-group semantics and run after a contiguous string is built in the background; cancellation takes effect after an engine call returns.
- Logical lines over 256 KiB and all subsequent content receive no syntax highlighting; text can still be edited and searched normally.
- Wrap indexes and line-change markers are stored by logical line. Highlighting far-away content and the initial indexing pass may require several seconds of background computation.
- `SearchEngine::find_all`, `find_all_rope`, and full-document highlighting APIs return all results; the interactive UI uses background viewport and paginated APIs.

## Regression checks

```sh
cargo test --locked -p app
cargo test --locked -p shell --lib editor
cargo test --locked -p shell --test editor
cargo test --locked -p ui --test editor
cargo check --workspace --all-targets --all-features --locked
python3 scripts/check-localization.py
```

`editor_scalability` covers equivalence between chunked and contiguous search, Unicode boundaries, dense pagination, cyclic navigation, stale tasks, replacement previews, long-line wrapping, and cache consistency after editing, undo, and width changes.
