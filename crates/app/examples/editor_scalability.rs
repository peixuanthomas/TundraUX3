//! Run each fixture in a fresh process. See scripts/benchmark-editor.py.
use app::editor::{EditorCommand, EditorState};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct MeasuredAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn add(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(live, Ordering::Relaxed);
}
unsafe impl GlobalAlloc for MeasuredAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            add(layout.size());
        }
        pointer
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            add(layout.size());
        }
        pointer
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let result = unsafe { System.realloc(pointer, layout, size) };
        if !result.is_null() {
            if size >= layout.size() {
                add(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        result
    }
}
#[global_allocator]
static ALLOCATOR: MeasuredAllocator = MeasuredAllocator;

fn fixture(kind: &str, bytes: usize) -> String {
    let unit = match kind {
        "ascii-single" => "abcdefghijklmnopqrstuvwxyz ",
        "ascii-lines" => "2026-10-13 INFO service ready key=value\n",
        "chinese" => "系统服务已启动，配置加载完成。日志记录正常。\n",
        "emoji" => "👩‍💻🇨🇳e\u{301}👍🏽 status ready\n",
        "crlf" => "2026-10-13 INFO service ready key=value\r\n",
        "dense" => "a ",
        _ => panic!("unknown fixture"),
    };
    let mut text = String::with_capacity(bytes);
    while text.len() + unit.len() <= bytes {
        text.push_str(unit);
    }
    text.extend(std::iter::repeat_n('x', bytes - text.len()));
    text
}
fn finish(state: &mut EditorState) {
    let deadline = Instant::now() + Duration::from_secs(180);
    while state.has_background_work() {
        state.poll_background();
        assert!(
            Instant::now() < deadline,
            "background computation timed out"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}
fn measure(
    state: &mut EditorState,
    fixture: &str,
    mib: usize,
    operation: &str,
    action: impl FnOnce(&mut EditorState),
) {
    let baseline = LIVE.load(Ordering::Relaxed);
    PEAK.store(baseline, Ordering::Relaxed);
    let start = Instant::now();
    action(state);
    let request = start.elapsed();
    finish(state);
    let elapsed = start.elapsed();
    let peak = PEAK.load(Ordering::Relaxed);
    println!(
        "{fixture},{mib},{operation},{:.3},{:.3},{:.3},{:.3}",
        request.as_secs_f64() * 1000.,
        elapsed.as_secs_f64() * 1000.,
        peak.saturating_sub(baseline) as f64 / 1048576.,
        peak as f64 / 1048576.
    );
}
fn main() {
    let arguments: Vec<_> = std::env::args().collect();
    let kind = arguments.get(1).map_or("ascii-single", String::as_str);
    let mib: usize = arguments
        .get(2)
        .map_or("1", String::as_str)
        .parse()
        .unwrap();
    let text = fixture(kind, mib * 1024 * 1024);
    let mut state = EditorState::open_owned("benchmark.c", text.into_bytes()).unwrap();
    measure(&mut state, kind, mib, "syntax-first", |state| {
        state.source_syntax_highlights_in(0..40);
    });
    let count = state.source_line_count().unwrap();
    measure(&mut state, kind, mib, "syntax-end", |state| {
        state.source_syntax_highlights_in(count.saturating_sub(40)..count);
    });
    measure(&mut state, kind, mib, "wrap", |state| {
        state.source_wrap_layout(80);
    });
    measure(&mut state, kind, mib, "search", |state| {
        state.apply(EditorCommand::Find {
            query: if kind == "dense" {
                "a"
            } else {
                "never-present-needle"
            }
            .into(),
            options: Default::default(),
            backwards: false,
        });
    });
    measure(&mut state, kind, mib, "edit-reflow", |state| {
        state.apply(EditorCommand::InsertText("x".into()));
        state.source_wrap_layout(80);
        state.source_syntax_highlights_in(0..40);
        state.source_line_markers();
    });
    // OS RSS includes allocator retention, stacks and native allocations.
    #[cfg(target_os = "linux")]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        if let Some(line) = status.lines().find(|line| line.starts_with("VmHWM:")) {
            eprintln!("{kind},{mib},{line}");
        }
    }
}
