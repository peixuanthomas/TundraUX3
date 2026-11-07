# weathr

`weathr` is a weather-scene rendering library used for weather applications and idle screens on all platforms, and also for lock screens on Windows/macOS. It reads shared snapshots, does not start system services, and does not own the full-screen terminal lifecycle.

## Embedded rendering

`EmbeddedDisplay::new` accepts a weather snapshot, a translation function, and dimensions; `render` returns a Ratatui Buffer for one frame and supports resizing. It does not read terminal input, switch terminal modes, or write directly to the terminal.

Weather, day/night, season, and animation together determine scenes such as houses, trees, clouds, rain and snow, and moon phases. Weather data and caching are provided by [system-services](../system-services/README.md), and Shell can prefetch them at startup.

Shell uses the same rendering code for the weather application, idle screen, and lock screen. Background tasks continue while idle, and a key press or tap returns to the previous page; Shell manages authentication and page state.

## Standalone lock screen and control

The lock screen supports 12/24-hour time, terminal resizing, and entry with any key or click. The host handles watchdog, terminal modes, and the panic page.

`run_display_blocking_with_control` accepts `DisplayControl`, allowing the host to read rendered frames and send normal terminal input. Shell uses it to support external CLI access to weather screens and `wake`; Shell and platform still manage connections, the enable switch, and permissions.

## Validation

```sh
cargo test --locked -p weathr
bash scripts/check-weathr-dependency-boundary.sh
```

The dependency-boundary check ensures that data rendering does not introduce a reverse dependency on Shell or the system runtime.
