use super::*;

#[derive(Default)]
struct CountingWriter {
    bytes: Vec<u8>,
    writes: usize,
    flushes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        self.bytes.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

fn assets() -> ui::RuntimeAsciiAssets {
    let asset_root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    let store =
        ui::AsciiAssetStore::load_with_root(asset_root, "default").expect("canonical ASCII assets");
    ui::RuntimeAsciiAssets::from_store(store)
}

#[test]
fn matrix_frame_is_submitted_to_the_terminal_in_one_write() {
    let frame = vec![
        vec![
            Some(MatrixCell {
                glyph: 'A',
                tone: MatrixTone::Green,
            }),
            None,
        ],
        vec![
            None,
            Some(MatrixCell {
                glyph: 'B',
                tone: MatrixTone::White,
            }),
        ],
    ];
    let mut output = CountingWriter::default();
    let mut terminal_buffer = String::new();

    render_matrix_frame(&mut output, &frame, &mut terminal_buffer).expect("rendered frame");

    assert_eq!(output.writes, 1);
    assert_eq!(output.flushes, 1);
    assert_eq!(output.bytes, terminal_buffer.as_bytes());
}

#[test]
fn zero_timing_sequence_renders_white_banner_and_resets_the_terminal() {
    let assets = assets();
    let first_visible_line = assets
        .banner_lines(BANNER_ASSET_KEY)
        .unwrap()
        .iter()
        .find(|line| !line.trim().is_empty())
        .unwrap();
    let mut output = Vec::new();

    display_first_run_banner_with_timing_and_size_check(
        &mut output,
        &assets,
        MatrixTiming::ZERO,
        Color::White,
        || Ok((120, 40)),
    )
    .expect("zero-duration Matrix sequence");

    let output = String::from_utf8(output).unwrap();
    assert!(output.contains(&MatrixTone::Banner(Color::White).ansi()));
    assert!(output.contains(first_visible_line.trim_end()));
    assert!(output.ends_with(&format!("{RESET_STYLE}{CLEAR_SCREEN}")));
}

#[test]
fn matrix_animation_stops_when_terminal_size_check_fails() {
    let assets = assets();
    let mut output = Vec::new();
    let mut checks = 0;

    let error = display_first_run_banner_with_timing_and_size_check(
        &mut output,
        &assets,
        MatrixTiming::ZERO,
        Color::White,
        || {
            checks += 1;
            if checks >= 2 {
                Err(io::Error::other("terminal became too small"))
            } else {
                Ok((120, 40))
            }
        },
    )
    .expect_err("failed size check stops the Matrix sequence");

    assert!(error.to_string().contains("too small"));
    assert!(!String::from_utf8(output).unwrap().ends_with(CLEAR_SCREEN));
}
