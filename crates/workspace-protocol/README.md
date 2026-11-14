# workspace-protocol

Private IPC messages and framing for the workspace guardian, Shell, app hosts, and shared services. The crate depends on neither terminal rendering nor application state. The external CLI `ui` commands continue to use [shell-control](../shell-control/README.md).

| Connection | Messages |
| --- | --- |
| Guardian and workers | Identity handshake, event-loop progress, probes, process state, shutdown, and Windows desktop opens |
| Shell and app host | Window initialization, viewport, input, close, and rendered frames |
| Worker and shared services | Request IDs, configuration, profile, authorization policy, file operations, and system status |
| Embedded Linux CLI and guardian | Ancestry-checked resource registration and retirement |

`RunId` identifies one guardian invocation. `SessionId` identifies one app opening and is never reused within that run. `WorkerId` includes the process generation so that delayed close and kill requests cannot affect another worker. Window coordinates count terminal cells from the content area's top left. `LayoutGeneration` binds viewport-dependent input and frames to one layout. App minimums are content dimensions, with a baseline of 108 × 18 cells.

Each message has a four-byte big-endian length and a JSON body. Frame limits are 1 MiB for guardian traffic, 128 MiB for windows, and 8 MiB for shared services. Window cells carry displayable text and style in a gzip-compressed, base64-encoded JSON array, with limits of 256 Ki cells and 16 MiB of decompressed data. Image placements identify their visible regions and refer to the base64-encoded RGBA data from the same frame or the previous frame. The image limit is 80 MB of unique pixel data per window frame. Cursors, controls, and masks use structured data. `Framed` expects an authenticated nonblocking stream from the platform layer and bounds queued bytes and work per poll. Queue backpressure is reported as `WouldBlock`.

Window frames also carry styled Markdown headings as text and geometry. The receiving Shell can use native text sizing after checking the pane's bounds.

Configuration, profile, and authorization writes include the revision the caller read. File paths carried by `NativePath` use Unix bytes or Windows UTF-16 code units so they round trip even when they cannot be represented as UTF-8.

Validation: `cargo test --locked -p workspace-protocol`.
