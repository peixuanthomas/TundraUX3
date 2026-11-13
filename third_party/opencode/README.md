# OpenCode UI reference

The terminal interface is based on the component designs in
[`anomalyco/opencode`](https://github.com/anomalyco/opencode/tree/5b131f5995a9f7fad458c59a7a5e39686237beb8/packages/tui/src),
commit `5b131f5995a9f7fad458c59a7a5e39686237beb8` (2026-11-13).

`crates/ui/src/components/busy_indicator.rs` adapts the 1200ms loading-text
sweep from `component/shimmer-text.tsx` and the `coast`, `intensityAt`, and
`smootherstep` curves from `component/tab-pulse.tsx`. It uses Ratatui terminal
columns and TundraUX3's existing motion clock. The ten Braille spinner frames
and 80ms cadence match `component/spinner-frames.ts` and `component/spinner.tsx`.
The upstream MIT license is preserved in [LICENSE](LICENSE).
Current release packages include it as `LICENSE.opencode`.

Dialog, selection, and sidebar layouts are implemented in Rust using existing
TundraUX3 controls. They follow the upstream visual structure while retaining
TundraUX3's Glacier Night palette, localization, pointer-release activation,
and application workflows. OpenCode's mini-interface spinner variants and
session-title renaming effects have no corresponding workflow here.

Source links and the adaptation contract are recorded in
[UI requirements](../../docs/UI-requirements.md#opencode-source-reference).
