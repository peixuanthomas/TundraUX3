use super::*;
use ratatui::{Terminal, backend::TestBackend, style::Style};
#[test]
fn rgba_preparation_rejects_wrong_buffer_length() {
    let error = rgba_image(2, 3, vec![0; 23]).unwrap_err();
    assert!(matches!(
        error,
        EditorMediaError::InvalidRgbaLength {
            width: 2,
            height: 3,
            expected: 24,
            actual: 23,
        }
    ));
}

#[test]
fn pure_capabilities_select_protocols_and_measured_render_geometry() {
    for protocol in [
        EditorGraphicsProtocol::Kitty,
        EditorGraphicsProtocol::Sixel,
        EditorGraphicsProtocol::Iterm2,
    ] {
        let probe = TerminalGraphicsProbe::from_terminal_capabilities(protocol, 5, 10, true, false);
        let prepared = probe
            .picker()
            .unwrap()
            .prepare(DynamicImage::new_rgba8(100, 100), Rect::new(0, 0, 40, 40))
            .unwrap();
        assert_eq!(prepared.protocol(), Some(protocol));
        assert_eq!(prepared.render_size(), Size::new(20, 10));
        assert!(matches!(
            (&prepared.image, protocol),
            (
                PreparedImage::Native(Protocol::Kitty(_)),
                EditorGraphicsProtocol::Kitty
            ) | (
                PreparedImage::Native(Protocol::Sixel(_)),
                EditorGraphicsProtocol::Sixel
            ) | (
                PreparedImage::Native(Protocol::ITerm2(_)),
                EditorGraphicsProtocol::Iterm2
            )
        ));
    }

    let default = TerminalGraphicsProbe::from_terminal_capabilities(
        EditorGraphicsProtocol::Kitty,
        10,
        20,
        false,
        false,
    );
    let prepared = default
        .picker()
        .unwrap()
        .prepare(DynamicImage::new_rgba8(100, 100), Rect::new(0, 0, 40, 40))
        .unwrap();
    assert_eq!(prepared.render_size(), Size::new(10, 5));
}

#[test]
fn workspace_icons_center_and_capture_the_same_pixels_as_the_fallback() {
    let picker = EditorImagePicker::for_workspace(2, 4);
    let area = Rect::new(3, 3, 10, 4);
    let content = Rect::new(2, 2, 12, 8);
    let pixels = [220, 100, 40, 128].repeat(8 * 8);
    let image = picker.prepare_rgba(8, 8, pixels, area).unwrap();
    assert_eq!(image.render_size(), Size::new(4, 2));
    let mut terminal = Terminal::new(TestBackend::new(16, 10)).unwrap();
    let expected = workspace_protocol::Rect {
        x: 4,
        y: 2,
        width: 4,
        height: 2,
    };
    let mut previous = None;
    for background in [Color::Black, Color::Black, Color::Blue] {
        let capture = WorkspaceIconCapture::begin(content);
        terminal
            .draw(|frame| {
                frame
                    .buffer_mut()
                    .set_style(area, Style::default().bg(background));
                image.render_centered(frame, area);
            })
            .unwrap();
        let images = capture.finish();
        let [asset] = &images[..] else {
            panic!("expected one icon placement")
        };
        assert_eq!(asset.area, expected);
        assert_eq!(asset.visible, [expected]);
        assert_eq!([asset.width, asset.height], [8, 8]);
        assert_eq!(asset.image_id >> 63, 1);
        let cell = &terminal.backend().buffer()[(6, 4)];
        assert!(!cell.symbol().contains(char::is_control));
        let rgb = crate::theme::color_rgb(cell.bg).unwrap();
        assert_eq!(asset.rgba[..4], [rgb.0, rgb.1, rgb.2, 255]);
        let key = (background, asset.image_id);
        if let Some((old_background, old_id)) = previous {
            assert_eq!(old_background == background, old_id == asset.image_id);
        }
        previous = Some(key);
    }
}

#[test]
fn workspace_icon_stays_inside_the_host_pane_when_encoded_natively() {
    let mut worker = Terminal::new(TestBackend::new(14, 8)).unwrap();
    let picker = EditorImagePicker::for_workspace(2, 4);
    let area = Rect::new(2, 2, 8, 4);
    let image = picker
        .prepare_rgba(8, 8, [25, 100, 200, 255].repeat(64), area)
        .unwrap();
    let capture = WorkspaceIconCapture::begin(Rect::new(0, 1, 14, 6));
    worker
        .draw(|frame| image.render_centered(frame, area))
        .unwrap();
    let images = capture.finish();
    let native =
        EditorImagePicker::from_terminal_capabilities(EditorGraphicsProtocol::Iterm2, 2, 4, false);
    let mut host = Terminal::new(TestBackend::new(30, 12)).unwrap();
    let pane = Rect::new(10, 2, 14, 6);
    let mut cache = crate::TerminalImageCache::default();
    host.draw(|frame| {
        frame.buffer_mut()[(13, 4)].set_symbol("N");
        frame.buffer_mut()[(18, 4)].set_symbol("M");
        cache.render_workspace(frame, pane, &images, &native);
    })
    .unwrap();
    let buffer = host.backend().buffer();
    assert!(buffer[(14, 4)].symbol().contains("\x1b]1337;File="));
    assert_eq!(buffer[(13, 4)].symbol(), "N");
    assert_eq!(buffer[(18, 4)].symbol(), "M");
}

#[test]
fn protocol_footprint_is_centered_and_clamped_inside_its_allocation() {
    assert_eq!(
        centered_protocol_area(
            Rect::new(10, 5, 20, 6),
            Size {
                width: 8,
                height: 4,
            },
        ),
        Rect::new(16, 6, 8, 4)
    );
    assert_eq!(
        centered_protocol_area(
            Rect::new(10, 5, 4, 2),
            Size {
                width: 8,
                height: 4,
            },
        ),
        Rect::new(10, 5, 4, 2)
    );
}
