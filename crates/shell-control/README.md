# shell-control

Defines the JSON requests, responses, interface trees, and snapshots shared by the external CLI and a running Shell. This crate does not read configuration, connect to terminals, or perform system operations. Coordinates use terminal character cells, with `(0, 0)` at the top left.

Connections are provided by platform. Shell publishes snapshots after actual rendering and converts requests into normal keyboard, mouse, and paste input. The protocol version is 1, and error codes do not change with the language.

A `wake` request closes idle weather or the weather lock screen and returns the subsequent Shell screen. On ordinary pages, it only returns the current snapshot without sending a key.

Snapshot numbers increase with rendering, but automatic updates do not invalidate old numbers. After real user actions, Shell rejects control requests until a snapshot is read again; explicitly specifying a number from before the user action is still rejected, except on weather screens.

Validation: `cargo test --locked -p shell-control`.
