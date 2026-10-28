use super::*;
use crate::{CommandLineCell, TerminalGraphicsProbe};
use ratatui::{Terminal, backend::TestBackend, style::Color};

fn fragment() -> TerminalImageFragment {
    TerminalImageFragment {
        image_id: 42,
        width: 4,
        height: 2,
        rgba: Arc::from([
            255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 0,
            0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
        ]),
        source: [0, 0, 4, 2],
        padding: [0; 4],
        cell_size: [2, 2],
        area: Rect::new(1, 1, 2, 1),
        z_index: 0,
    }
}

fn picker(protocol: EditorGraphicsProtocol, tmux: bool) -> EditorImagePicker {
    TerminalGraphicsProbe::from_terminal_capabilities(protocol, 8, 16, tmux, false)
        .picker()
        .unwrap()
        .clone()
}

#[test]
fn fallback_crops_pixels_to_the_visible_terminal_and_applies_the_host_offset() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(12, 8)).unwrap();
    screen
        .draw(|frame| cache.render(frame, Rect::new(5, 3, 2, 2), &snapshot, None))
        .unwrap();
    let buffer = screen.backend().buffer();
    assert_eq!(buffer[(6, 4)].bg, Color::Rgb(255, 0, 0));
    assert_eq!(buffer[(7, 4)].bg, Color::Reset);
    assert_eq!(buffer[(5, 4)].bg, Color::Reset);
    assert_eq!(buffer[(6, 3)].bg, Color::Reset);
}

#[test]
fn background_images_leave_text_and_wide_continuations_visible() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    let mut image = fragment();
    image.area = Rect::new(0, 0, 4, 2);
    image.z_index = -1;
    snapshot.images.push(image);
    snapshot.set_cell(
        1,
        0,
        CommandLineCell {
            symbol: "界".into(),
            ..Default::default()
        },
    );
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(1, 0)].set_symbol("界");
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    let buffer = screen.backend().buffer();
    assert_eq!(buffer[(1, 0)].symbol(), "界");
    assert_eq!(buffer[(2, 0)].bg, Color::Reset);
    assert_ne!(buffer[(0, 1)].bg, Color::Reset);
}

#[test]
fn native_backends_encode_only_the_clipped_picture_area() {
    for (protocol, prefix) in [
        (EditorGraphicsProtocol::Kitty, "\x1b_G"),
        (EditorGraphicsProtocol::Sixel, "\x1bP"),
        (EditorGraphicsProtocol::Iterm2, "\x1b]1337;File=inline=1"),
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
        snapshot.images.push(fragment());
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(12, 8)).unwrap();
        screen
            .draw(|frame| {
                cache.render(
                    frame,
                    Rect::new(5, 3, 2, 2),
                    &snapshot,
                    Some(&picker(protocol, false)),
                )
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        assert!(buffer[(6, 4)].symbol().contains(prefix), "{protocol:?}");
        assert!(!buffer[(7, 4)].symbol().contains('\x1b'));
        assert!(!buffer[(5, 4)].symbol().contains('\x1b'));
    }
}

#[test]
fn clearing_or_replacing_a_picture_retires_only_its_owned_native_resource() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let old = cache.entries.values().next().unwrap().kitty.unwrap().0;
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert!(cache.take_cleanup().is_empty());
    snapshot.images[0].rgba = Arc::from([255_u8; 32]);
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let new = cache.entries.values().next().unwrap().kitty.unwrap().0;
    assert_ne!(old, new);
    assert_eq!(
        cache.take_cleanup(),
        format!("\x1b_Ga=d,d=I,i={old}\x1b\\").as_bytes()
    );
    snapshot.images.clear();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert!(cache.entries.is_empty());
    assert_eq!(
        cache.take_cleanup(),
        format!("\x1b_Ga=d,d=I,i={new}\x1b\\").as_bytes()
    );
    assert_eq!(screen.backend().buffer()[(1, 1)].symbol(), " ");
}

#[test]
fn tmux_cleanup_wraps_only_generated_resource_identifiers() {
    let mut cleanup = Vec::new();
    append_cleanup(&mut cleanup, Some((19, true)));
    assert_eq!(cleanup, b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=19\x1b\x1b\\\x1b\\");
}

#[test]
fn malformed_or_oversized_picture_data_does_not_change_the_screen() {
    for image in [
        TerminalImageFragment {
            rgba: Arc::from([0_u8; 4]),
            ..fragment()
        },
        TerminalImageFragment {
            source: [4, 0, 1, 1],
            ..fragment()
        },
        TerminalImageFragment {
            width: u32::MAX,
            height: u32::MAX,
            ..fragment()
        },
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
        snapshot.images.push(image);
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
        screen
            .draw(|frame| cache.render(frame, frame.area(), &snapshot, None))
            .unwrap();
        assert!(cache.entries.is_empty());
        assert!(
            screen
                .backend()
                .buffer()
                .content()
                .iter()
                .all(|cell| cell.symbol() == " ")
        );
    }
}

fn offset_picture() -> TerminalImageFragment {
    TerminalImageFragment {
        image_id: 100,
        width: 17,
        height: 6,
        rgba: Arc::from([255, 0, 0, 255].repeat(17 * 6)),
        source: [0, 0, 17, 6],
        padding: [3, 4, 4, 6],
        cell_size: [8, 16],
        area: Rect::new(0, 0, 3, 1),
        z_index: 0,
    }
}

#[test]
fn native_pixel_canvas_preserves_non_cell_size_and_xy_offsets() {
    let image = offset_picture();
    let canvas = image_canvas(&image).unwrap();
    assert_eq!(canvas.dimensions(), (24, 16));
    assert_eq!(canvas.get_pixel(3, 4).0, [255, 0, 0, 255]);
    assert_eq!(canvas.get_pixel(19, 9).0, [255, 0, 0, 255]);
    for (x, y) in [(2, 4), (3, 3), (20, 4), (3, 10), (23, 15)] {
        assert_eq!(canvas.get_pixel(x, y)[3], 0, "{x},{y}");
    }
    assert_eq!(
        source_for_area(&image, Rect::new(1, 0, 1, 1)),
        Some([8, 0, 8, 16])
    );
    let clipped = image::imageops::crop_imm(&canvas, 8, 0, 8, 16).to_image();
    assert_eq!(clipped.get_pixel(0, 4).0, [255, 0, 0, 255]);
    assert_eq!(clipped.get_pixel(7, 9).0, [255, 0, 0, 255]);
    assert_eq!(clipped.get_pixel(0, 10)[3], 0);
}

#[test]
fn viewport_and_scrollback_crop_the_padded_canvas_without_rescaling_it() {
    let image = TerminalImageFragment {
        height: 20,
        rgba: Arc::from([255, 0, 0, 255].repeat(17 * 20)),
        source: [0, 0, 17, 20],
        padding: [3, 4, 4, 8],
        area: Rect::new(0, 0, 3, 2),
        ..offset_picture()
    };
    let canvas = image_canvas(&image).unwrap();
    assert_eq!(
        source_for_area(&image, Rect::new(1, 1, 2, 1)),
        Some([8, 16, 16, 16])
    );
    let cropped = image::imageops::crop_imm(&canvas, 8, 16, 16, 16).to_image();
    assert_eq!(cropped.get_pixel(11, 7).0, [255, 0, 0, 255]);
    assert_eq!(cropped.get_pixel(12, 7)[3], 0);
    assert_eq!(cropped.get_pixel(0, 8)[3], 0);
}

#[test]
fn fallback_composites_alpha_over_the_existing_background_and_preserves_empty_pixels() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(1, 1);
    snapshot.images.push(TerminalImageFragment {
        width: 2,
        height: 2,
        rgba: Arc::from([255, 0, 0, 128].repeat(4)),
        source: [0, 0, 2, 2],
        area: Rect::new(0, 0, 1, 1),
        ..fragment()
    });
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(1, 1)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(0, 0)].set_bg(Color::Rgb(0, 0, 255));
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    let Color::Rgb(red, green, blue) = screen.backend().buffer()[(0, 0)].bg else {
        panic!("RGB fallback")
    };
    assert!((127..=128).contains(&red));
    assert_eq!(green, 0);
    assert!((126..=128).contains(&blue));
    snapshot.images[0].rgba = Arc::from([255, 0, 0, 0].repeat(4));
    screen
        .draw(|frame| {
            frame.buffer_mut()[(0, 0)]
                .set_symbol("X")
                .set_bg(Color::Rgb(4, 5, 6));
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    assert_eq!(screen.backend().buffer()[(0, 0)].symbol(), "X");
    assert_eq!(screen.backend().buffer()[(0, 0)].bg, Color::Rgb(4, 5, 6));
}

#[test]
fn fallback_uses_source_cell_geometry_for_partial_edge_cells() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(3, 1);
    snapshot.images.push(TerminalImageFragment {
        height: 16,
        rgba: Arc::from([255, 0, 0, 255].repeat(17 * 16)),
        source: [0, 0, 17, 16],
        padding: [3, 0, 4, 0],
        ..offset_picture()
    });
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(3, 1)).unwrap();
    screen
        .draw(|frame| {
            for x in 0..3 {
                frame.buffer_mut()[(x, 0)].set_bg(Color::Rgb(0, 0, 255));
            }
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    let buffer = screen.backend().buffer();
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(255, 0, 0));
    for x in [0, 2] {
        let Color::Rgb(red, green, blue) = buffer[(x, 0)].bg else {
            panic!("RGB fallback")
        };
        assert!(red > 0 && blue > 0);
        assert_eq!(green, 0);
    }
}

#[test]
fn native_transparent_holes_preserve_text_without_protocol_skip_cells() {
    for protocol in [
        EditorGraphicsProtocol::Kitty,
        EditorGraphicsProtocol::Sixel,
        EditorGraphicsProtocol::Iterm2,
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(2, 1);
        snapshot.images.push(TerminalImageFragment {
            width: 2,
            height: 2,
            rgba: Arc::from([0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255]),
            source: [0, 0, 2, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 2, 1),
            ..fragment()
        });
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(2, 1)).unwrap();
        screen
            .draw(|frame| {
                frame.buffer_mut()[(0, 0)].set_symbol("X");
                cache.render(
                    frame,
                    frame.area(),
                    &snapshot,
                    Some(&picker(protocol, false)),
                );
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "X", "{protocol:?}");
        assert_eq!(
            raster_background(&buffer[(1, 0)]),
            [Color::Rgb(255, 0, 0); 2]
        );
        assert!(
            cache
                .entries
                .values()
                .all(|entry| matches!(entry.protocol, Protocol::Halfblocks(_)))
        );
        assert!(buffer.content().iter().all(|cell| {
            !cell.symbol().contains('\x1b')
                && cell.diff_option != ratatui::buffer::CellDiffOption::Skip
        }));
    }
}

#[test]
fn overlapping_native_images_keep_both_layers_when_their_payload_anchors_coincide() {
    for protocol in [
        EditorGraphicsProtocol::Kitty,
        EditorGraphicsProtocol::Sixel,
        EditorGraphicsProtocol::Iterm2,
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(2, 1);
        snapshot.images.push(TerminalImageFragment {
            width: 2,
            height: 2,
            rgba: Arc::from([255, 0, 0, 255].repeat(4)),
            source: [0, 0, 2, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 2, 1),
            ..fragment()
        });
        snapshot.images.push(TerminalImageFragment {
            image_id: 43,
            width: 1,
            height: 2,
            rgba: Arc::from([0, 0, 255, 255].repeat(2)),
            source: [0, 0, 1, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 1, 1),
            z_index: 1,
            ..fragment()
        });
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(2, 1)).unwrap();
        screen
            .draw(|frame| {
                cache.render(
                    frame,
                    frame.area(),
                    &snapshot,
                    Some(&picker(protocol, false)),
                );
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        assert_eq!(
            raster_background(&buffer[(0, 0)]),
            [Color::Rgb(0, 0, 255); 2]
        );
        assert_eq!(
            raster_background(&buffer[(1, 0)]),
            [Color::Rgb(255, 0, 0); 2]
        );
        assert!(
            buffer
                .content()
                .iter()
                .all(|cell| !cell.symbol().contains('\x1b')),
            "{protocol:?}"
        );
    }
}

#[test]
fn translucent_layers_blend_each_half_cell_and_leave_transparent_lower_pixels_visible() {
    for protocol in [
        EditorGraphicsProtocol::Kitty,
        EditorGraphicsProtocol::Sixel,
        EditorGraphicsProtocol::Iterm2,
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(1, 1);
        snapshot.images.push(TerminalImageFragment {
            width: 1,
            height: 2,
            rgba: Arc::from([255, 0, 0, 255, 0, 0, 255, 255]),
            source: [0, 0, 1, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 1, 1),
            ..fragment()
        });
        snapshot.images.push(TerminalImageFragment {
            image_id: 43,
            rgba: Arc::from([0, 255, 0, 128, 0, 0, 0, 0]),
            z_index: 1,
            ..snapshot.images[0].clone()
        });
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(1, 1)).unwrap();
        screen
            .draw(|frame| {
                cache.render(
                    frame,
                    frame.area(),
                    &snapshot,
                    Some(&picker(protocol, false)),
                );
            })
            .unwrap();
        let [upper, lower] = raster_background(&screen.backend().buffer()[(0, 0)]);
        let Color::Rgb(red, green, blue) = upper else {
            panic!("RGB raster")
        };
        assert!((126..=128).contains(&red), "{protocol:?}: {upper:?}");
        assert!((127..=128).contains(&green));
        assert_eq!(blue, 0);
        assert_eq!(lower, Color::Rgb(0, 0, 255));
    }
}

#[test]
fn preparation_rejects_large_temporary_canvases_before_allocating_or_encoding() {
    let image = TerminalImageFragment {
        width: 1,
        height: 1,
        rgba: Arc::from([255, 0, 0, 255]),
        source: [0, 0, 1, 1],
        cell_size: [1024, 1024],
        area: Rect::new(0, 0, 4, 4),
        ..fragment()
    };
    let (stored, working) = preparation_cost(&image, Size::new(4, 4), (8, 16)).unwrap();
    assert!(stored + working > CACHE_BUDGET);
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 4);
    snapshot.images.push(image);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 4)).unwrap();
    screen
        .draw(|frame| {
            cache.render(
                frame,
                frame.area(),
                &snapshot,
                Some(&picker(EditorGraphicsProtocol::Kitty, false)),
            );
        })
        .unwrap();
    assert!(cache.entries.is_empty());
    assert!(
        screen
            .backend()
            .buffer()
            .content()
            .iter()
            .all(|cell| cell.symbol() == " ")
    );
}

#[test]
fn preparation_bounds_the_float_resize_pass_as_well_as_final_pixels() {
    let image = TerminalImageFragment {
        width: 8192,
        height: 1,
        rgba: Arc::from([255, 0, 0, 255].repeat(8192)),
        source: [0, 0, 8192, 1],
        cell_size: [1, 4096],
        area: Rect::new(0, 0, 1, 1),
        ..fragment()
    };
    assert!(preparation_cost(&image, Size::new(1, 1), (8, 16)).is_none());
}

#[test]
fn preparing_a_picture_evicts_unused_cached_resources_before_the_new_allocation() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let old = cache.entries.values().next().unwrap().kitty.unwrap().0;
    cache.entries.values_mut().next().unwrap().bytes = CACHE_BUDGET - snapshot.images[0].rgba.len();
    snapshot.images[0].rgba = Arc::from([0, 0, 255, 255].repeat(8));
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert_eq!(cache.entries.len(), 1);
    assert!(cache.cached_bytes() <= CACHE_BUDGET);
    assert_ne!(cache.entries.values().next().unwrap().kitty.unwrap().0, old);
    assert_eq!(
        cache.take_cleanup(),
        format!("\x1b_Ga=d,d=I,i={old}\x1b\\").as_bytes()
    );
}

#[test]
fn pictures_already_used_this_frame_count_against_later_preparation() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let old = cache.entries.values().next().unwrap().kitty.unwrap().0;
    cache.entries.values_mut().next().unwrap().bytes = CACHE_BUDGET - snapshot.images[0].rgba.len();
    snapshot.images.push(TerminalImageFragment {
        image_id: 43,
        rgba: Arc::from([0, 0, 255, 255].repeat(8)),
        area: Rect::new(0, 0, 1, 1),
        z_index: 1,
        ..fragment()
    });
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.cached_bytes(), CACHE_BUDGET);
    assert_eq!(cache.entries.values().next().unwrap().kitty.unwrap().0, old);
    assert!(cache.take_cleanup().is_empty());
    assert_eq!(screen.backend().buffer()[(0, 0)].symbol(), " ");
}

#[test]
fn raster_images_remove_inherited_skip_and_inverse_styles() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(TerminalImageFragment {
        rgba: Arc::from([255, 0, 0, 255].repeat(8)),
        ..fragment()
    });
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(1, 1)]
                .set_style(ratatui::style::Style::new().reversed())
                .set_diff_option(ratatui::buffer::CellDiffOption::Skip);
            cache.render(frame, frame.area(), &snapshot, None);
            assert_eq!(
                frame.buffer_mut()[(1, 1)].diff_option,
                ratatui::buffer::CellDiffOption::None
            );
            assert!(
                !frame.buffer_mut()[(1, 1)]
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        })
        .unwrap();
    assert_eq!(
        raster_background(&screen.backend().buffer()[(1, 1)]),
        [Color::Rgb(255, 0, 0); 2]
    );
}

#[test]
fn deeply_negative_images_respect_the_effective_background_of_inverse_cells() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(TerminalImageFragment {
        z_index: i32::MIN,
        ..fragment()
    });
    snapshot.set_cell(
        1,
        1,
        CommandLineCell {
            style: crate::CommandLineCellStyle {
                foreground: CommandLineColor::Rgb(0, 0, 255),
                inverse: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(1, 1)]
                .set_fg(Color::Rgb(0, 0, 255))
                .set_style(ratatui::style::Style::new().reversed());
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    assert_eq!(
        raster_background(&screen.backend().buffer()[(1, 1)]),
        [Color::Rgb(0, 0, 255); 2]
    );
    assert_ne!(screen.backend().buffer()[(2, 1)].bg, Color::Reset);
}
