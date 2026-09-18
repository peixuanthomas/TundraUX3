use super::*;
use watchdog::{ManagedThreadHandle, WatchdogConfig, WatchdogRuntime};

struct Fixture {
    runtime: Option<WatchdogRuntime>,
    process: ProcessWatchdog,
    app: AppWatchdog,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tundra-logind-test-{}-{}",
            std::process::id(),
            DETACHED_TASK_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let config = WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "logind-test",
            "1",
        );
        let (runtime, process) = WatchdogRuntime::start_isolated(config).unwrap();
        let app = process
            .register_app(AppDescriptor::new(
                AppId::from_static("platform-linux"),
                "Linux platform",
                "1",
                AppCriticality::Optional,
            ))
            .unwrap();
        Self {
            runtime: Some(runtime),
            process,
            app,
            root,
        }
    }

    fn start<F, Fut>(
        &self,
        connect: F,
    ) -> (
        ManagedThreadHandle<()>,
        mpsc::Receiver<PlatformLifecycleEvent>,
    )
    where
        F: Fn() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = zbus::Result<zbus::Connection>>,
    {
        let (sender, receiver) = mpsc::channel();
        let handle = self
            .app
            .task_group("platform-linux")
            .spawn_cancellable_thread(
                TaskSpec::idempotent_service(
                    TaskId::new("logind-prepareforsleep").unwrap(),
                    RestartPolicy::limited(
                        3,
                        Duration::from_secs(60),
                        vec![Duration::from_secs(1)],
                    ),
                ),
                move |cancellation| {
                    run_logind_listener(
                        cancellation,
                        &sender,
                        "PrepareForSleep",
                        PlatformLifecycleEvent::PrepareForSleep,
                        Some(PlatformLifecycleEvent::Resumed),
                        &connect,
                    )
                },
            )
            .unwrap();
        (handle, receiver)
    }

    fn assert_clean_shutdown(&mut self, handle: ManagedThreadHandle<()>) {
        self.runtime.take().unwrap().shutdown().unwrap();
        assert!(
            handle.is_finished(),
            "logind worker must finish before watchdog shutdown"
        );
        assert_eq!(handle.join().unwrap(), None);
        assert!(
            self.process.try_recv_incident().is_none(),
            "normal exit must not create an incident"
        );
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown().unwrap();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn logind_shutdown_cancels_connection_setup_and_retry_delay() {
    for pending in [true, false] {
        let mut fixture = Fixture::new();
        let (started_tx, started_rx) = mpsc::channel();
        let (handle, _events) = fixture.start(move || {
            started_tx.send(()).unwrap();
            async move {
                if pending {
                    futures_lite::future::pending().await
                } else {
                    Err(zbus::Error::Failure("test bus unavailable".into()))
                }
            }
        });
        started_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        fixture.assert_clean_shutdown(handle);
    }
}

struct Bus(Child);
impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "requires dbus-daemon; uses a private bus, never host power actions"]
fn logind_signals_and_shutdown_work_on_idle_and_disconnected_bus() {
    for disconnect in [false, true] {
        let mut bus = Bus(Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap());
        let mut address = String::new();
        BufReader::new(bus.0.stdout.take().unwrap())
            .read_line(&mut address)
            .unwrap();
        let address = address.trim().to_owned();
        let server = zbus::blocking::connection::Builder::address(address.as_str())
            .unwrap()
            .name("org.freedesktop.login1")
            .unwrap()
            .build()
            .unwrap();
        let mut fixture = Fixture::new();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (handle, events) = fixture.start(move || {
            attempt_tx.send(()).unwrap();
            let builder = zbus::connection::Builder::address(address.as_str()).unwrap();
            async move { builder.build().await }
        });
        attempt_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // A received event proves subscription has completed; no startup sleeps.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            server
                .emit_signal(
                    None::<&str>,
                    "/org/freedesktop/login1",
                    "org.freedesktop.login1.Manager",
                    "PrepareForSleep",
                    &true,
                )
                .unwrap();
            if events.recv_timeout(Duration::from_millis(25)).is_ok() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "listener did not subscribe"
            );
        }
        server
            .emit_signal(
                None::<&str>,
                "/org/freedesktop/login1",
                "org.freedesktop.login1.Manager",
                "PrepareForSleep",
                &false,
            )
            .unwrap();
        loop {
            if events.recv_timeout(Duration::from_secs(2)).unwrap()
                == PlatformLifecycleEvent::Resumed
            {
                break;
            }
        }
        if disconnect {
            bus.0.kill().unwrap();
            bus.0.wait().unwrap();
            attempt_rx
                .recv_timeout(Duration::from_secs(3))
                .expect("listener must reconnect after a bus disconnect");
        }
        fixture.assert_clean_shutdown(handle);
    }
}
