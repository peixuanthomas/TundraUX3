use super::*;
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
        assert_eq!(prepared.protocol(), protocol);
        assert_eq!(prepared.render_size(), Size::new(20, 10));
        assert!(matches!(
            (&prepared.protocol, protocol),
            (Protocol::Kitty(_), EditorGraphicsProtocol::Kitty)
                | (Protocol::Sixel(_), EditorGraphicsProtocol::Sixel)
                | (Protocol::ITerm2(_), EditorGraphicsProtocol::Iterm2)
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
