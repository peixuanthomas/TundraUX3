# Tundra trusted greeter

Linux-only frontend for sessiond's protected terminal. The frontend renders
requests and returns user input; it never establishes a session or grants an
operation itself.

Launch as the dedicated non-login greeter account:

```text
tundra-greeter --channel-fd 3 [--locale en-US|zh-CN]
```

FD 3 must be one endpoint of a private stream Unix socketpair created by root.
The launcher passes only this endpoint to greeter, retains the other endpoint,
and closes every copy in the desktop/user descendants. Greeter checks the
kernel-reported peer UID, marks the channel close-on-exec, refuses root execution,
and disables privilege gain and core dumps. There is no public listener or
path-based channel fallback.

The service owns VT/display/input isolation and must finish switching to the
protected terminal and clear queued input before sending a request. The greeter
also drains pending terminal events on each incoming request. A private channel
alone does not prove that the display or keyboard is isolated. The service must
bind every response to its outstanding PAM prompt or one-shot authorization,
source bus sender, original UID/logind session and expiration.

`session_protocol::greeter` defines newline-delimited JSON frames, limited to
65,536 bytes including newline. PAM info/error messages are acknowledged with an
empty `PamResponse`; echo-on and echo-off prompts receive independent responses.
`Ready` is sent exactly once after the initial successful terminal draw.
`Complete` exits the frontend. A disconnected or malformed channel causes a
failure exit, never a successful login/unlock/authorization.

All page controls reuse the project's Dialog, Button, and TextInput components.
Echo-off TextInput mode masks rendering, redacts Debug output, and erases its
storage when replaced or dropped; the frontend transfers response ownership and
erases both response and serialized transport buffers after sending. Paste,
key repeat and modified activation chords are ignored. Consent defaults to
cancel on every request and after a resize changes available actions. If the
entire consent title/body cannot fit, confirmation is unavailable. A response
moves the frontend to a waiting state, preventing duplicate submissions.

Default English and the theme are compiled into the installed binary. Chinese
loads only `/usr/share/tundra/greeter/locales`, after checking all ancestors and
tree entries are root-owned, not group/other-writable, and not symlinks. Install
both bundled locale trees there; no HOME/XDG configuration or user asset lookup
is performed. PAM and service text must still originate in the protected service
and is filtered for terminal control and bidirectional override characters.

Run `cargo test -p tundra-greeter` and
`cargo test -p ui secret_tests`. Unit tests cover frame limits, strict messages,
PAM rounds, masking, consent cancellation and one-shot input, clipping safety,
and paste/repeat rejection. Actual PAM, DRM/VT and input-device isolation require
Linux integration tests through sessiond.
