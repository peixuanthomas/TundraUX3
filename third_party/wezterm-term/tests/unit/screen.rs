use super::*;
use wezterm_surface::SEQ_ZERO;

#[derive(Debug)]
struct Config;

impl TerminalConfiguration for Config {
    fn color_palette(&self) -> crate::color::ColorPalette {
        Default::default()
    }
}

fn wrapped_screen() -> Screen {
    let config: Arc<dyn TerminalConfiguration> = Arc::new(Config);
    let mut screen = Screen::new(
        TerminalSize {
            rows: 4,
            cols: 8,
            ..Default::default()
        },
        &config,
        false,
        SEQ_ZERO,
        config.bidi_mode(),
    );
    screen.lines = VecDeque::with_capacity(8);
    for index in 0..screen.lines.capacity() {
        screen.lines.push_back(Line::from_text(
            &format!("line{index}"),
            &CellAttributes::default(),
            SEQ_ZERO,
            None,
        ));
    }
    for _ in 0..3 {
        let first = screen.lines.pop_front().unwrap();
        screen.lines.push_back(first);
    }
    assert!(!screen.lines.as_slices().1.is_empty());
    screen
}

fn ranges(screen: &Screen) -> Vec<Range<usize>> {
    let split = screen.lines.as_slices().0.len();
    let len = screen.lines.len();
    vec![
        split - 1..split + 1,
        split..len,
        split + 1..len,
        0..0,
        split..split,
        len..len,
        len + 2..len + 4,
        0..len + 4,
    ]
}

#[test]
fn physical_line_ranges_read_across_a_wrapped_deque() {
    let screen = wrapped_screen();
    for range in ranges(&screen) {
        let expected: Vec<String> = screen
            .lines
            .iter()
            .skip(range.start)
            .take(range.end - range.start)
            .map(|line| line.as_str().into_owned())
            .collect();
        let mut called = false;
        screen.with_phys_lines(range.clone(), |lines| {
            called = true;
            let actual: Vec<String> = lines
                .iter()
                .map(|line| line.as_str().into_owned())
                .collect();
            assert_eq!(actual, expected, "{range:?}");
        });
        assert!(called);
    }
}

#[test]
fn physical_line_ranges_mutate_only_requested_wrapped_rows() {
    for range in ranges(&wrapped_screen()) {
        let mut screen = wrapped_screen();
        let mut expected: Vec<String> = screen
            .lines
            .iter()
            .map(|line| line.as_str().into_owned())
            .collect();
        for (index, line) in expected.iter_mut().enumerate() {
            if range.contains(&index) {
                line.replace_range(..1, "X");
            }
        }
        let mut called = false;
        screen.with_phys_lines_mut(range.clone(), |lines| {
            called = true;
            for line in lines {
                line.set_cell(0, Cell::new('X', CellAttributes::default()), SEQ_ZERO);
            }
        });
        assert!(called);
        let actual: Vec<String> = screen
            .lines
            .iter()
            .map(|line| line.as_str().into_owned())
            .collect();
        assert_eq!(actual, expected, "{range:?}");
    }
}
