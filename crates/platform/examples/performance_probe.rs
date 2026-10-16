//! Reproducible public-API probes; fixtures live in a temporary directory.
//! cargo run --release --locked -p platform --example performance_probe
#[cfg(target_os = "linux")]
mod probe {
    use platform::{Platform, ProcessSpec, linux::LinuxPlatform, management::*};
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::collections::BTreeMap;
    use std::hint::black_box;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::time::Instant;

    struct CountingAllocator;
    static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    #[global_allocator]
    static ALLOCATOR: CountingAllocator = CountingAllocator;
    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            unsafe { System.realloc(ptr, layout, size) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    fn measure(name: &str, mut work: impl FnMut()) {
        work(); // warm filesystem and library caches
        let mut samples = Vec::new();
        for _ in 0..7 {
            let allocations = ALLOCATIONS.load(Ordering::Relaxed);
            let start = Instant::now();
            work();
            let elapsed = start.elapsed();
            let count = ALLOCATIONS.load(Ordering::Relaxed) - allocations;
            samples.push((elapsed, count));
        }
        samples.sort_unstable();
        println!(
            "{name}: median_ms={:.3} allocations={}",
            samples[3].0.as_secs_f64() * 1000.0,
            samples[3].1
        );
    }

    struct Interaction;
    impl OperationInteraction for Interaction {
        fn emit(&mut self, event: OperationEvent) {
            black_box(event);
        }
        fn ask(
            &mut self,
            _: &str,
            _: &str,
            _: &[String],
            _: bool,
        ) -> Result<String, ManagementError> {
            panic!("a read-only scan must not request input")
        }
    }

    pub fn run() {
        let root = tempfile::tempdir().unwrap();
        let platform = LinuxPlatform;
        let cancelled = AtomicBool::new(false);
        let spec = ProcessSpec::new("/usr/bin/head").args(["-c", "4194304", "/dev/zero"]);
        measure("process_4mib", || {
            let output = platform.spawn_wait_cancellable(&spec, &|| false).unwrap();
            assert_eq!(output.code, Some(0));
            assert_eq!(output.stdout.bytes().len(), 4 * 1024 * 1024);
            black_box(output);
        });

        let log = root.path().join("events.log");
        let line = b"2026-10-11 informational record for allocation measurement\n";
        std::fs::write(&log, line.repeat(9 * 1024 * 1024 / line.len() + 1)).unwrap();
        let query = runtime_log::LogQuery {
            file_path: Some(log),
            limit: 200,
            ..Default::default()
        };
        measure("log_8mib_tail_200", || {
            let result = platform::query_log_file(&query, &cancelled);
            assert_eq!(result.events.len(), 200);
            black_box(result);
        });

        let document = root.path().join("document.bin");
        let bytes = vec![b'x'; 64 * 1024 * 1024];
        std::fs::write(&document, &bytes).unwrap();
        measure("fingerprint_64mib", || {
            black_box(platform::document_fingerprint(&document).unwrap());
        });
        let destination = root.path().join("saved.bin");
        measure("save_64mib_1k_chunks", || {
            black_box(
                platform::atomic_write_document_with(&destination, |writer| {
                    for chunk in bytes.chunks(1024) {
                        writer.write_all(chunk)?;
                    }
                    Ok(())
                })
                .unwrap(),
            );
        });

        let directory = root.path().join("directory");
        std::fs::create_dir(&directory).unwrap();
        for index in 0..3000 {
            std::fs::write(directory.join(format!("{index}.txt")), b"ordinary text\n").unwrap();
        }
        measure("directory_3000", || {
            let listing = platform.read_directory(&directory).unwrap();
            assert_eq!(listing.entries.len(), 3000);
            assert!(listing.warnings.is_empty());
            black_box(listing);
        });
        let command = ManagementCommand {
            kind: ManagementKind::Disks,
            action: "scan".into(),
            target: None,
            values: BTreeMap::from([("directory".into(), directory.display().to_string())]),
            identity: BTreeMap::new(),
        };
        let context = ExecutionContext {
            caller_pid: 0,
            caller_start_ticks: 0,
            actor_uid: unsafe { libc::geteuid() },
            helper_path: Default::default(),
        };
        measure("scan_3000", || {
            let summary =
                platform::management::execute(&command, &context, &mut Interaction, &cancelled)
                    .unwrap();
            assert!(summary.contains("3000 unique files"));
            assert!(summary.contains("0 entries could not be read"));
            black_box(summary);
        });
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    probe::run();
    #[cfg(not(target_os = "linux"))]
    eprintln!("Run this probe on Linux/WSL.");
}
