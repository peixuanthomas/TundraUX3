use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    HomeViewModel, ShellEntry,
    components::{GridDropSide, GridDropTarget},
};

fn model(count: usize) -> HomeViewModel {
    HomeViewModel::user(
        "User",
        "Now",
        (0..count)
            .map(|index| ShellEntry::new(format!("App {index}"), "Open"))
            .collect(),
    )
}

#[test]
fn home_drop_targets_cover_halves_row_gaps_and_empty_tail_slots() {
    let home = model(5);
    let main = Rect::new(3, 2, 110, 25);
    let layout = ui::home_layout(main, &home);
    assert_eq!(layout.columns, 4);
    let first = layout.items[0].area;
    let last = layout.items[4].area;
    assert_eq!(
        layout.drop_target((first.x, first.y)),
        Some(GridDropTarget {
            item_index: 0,
            side: GridDropSide::Before
        })
    );
    assert_eq!(
        layout
            .drop_target((first.right() - 1, first.y))
            .unwrap()
            .insertion_index(),
        1
    );
    assert_eq!(
        layout
            .drop_target((first.x, first.bottom()))
            .unwrap()
            .insertion_index(),
        4
    );
    assert_eq!(
        layout.drop_target((layout.content.right() - 1, last.y)),
        Some(GridDropTarget {
            item_index: 4,
            side: GridDropSide::After
        })
    );
    assert!(layout.drop_target((main.x, main.y)).is_none());
    let edit = ui::home_edit_area(main, &home).unwrap();
    assert!(layout.drop_target((edit.x, edit.y)).is_none());
}

#[test]
fn home_scrolled_drop_targets_use_absolute_indices_and_exclude_scrollbar() {
    let home = HomeViewModel::user_with_selection("User", "Now", model(30).entries().to_vec(), 29)
        .with_viewport_offset(25);
    let layout = ui::home_layout(Rect::new(3, 2, 56, 16), &home);
    let first = layout.items[0];
    assert!(first.index > 0);
    assert_eq!(
        layout
            .drop_target((first.area.x, first.area.y))
            .unwrap()
            .insertion_index(),
        first.index
    );
    let bar = layout.scrollbar.unwrap();
    for x in [bar.x - 1, bar.x, bar.x + 1] {
        assert!(layout.drop_target((x, bar.y)).is_none());
    }
}

#[test]
fn home_edit_controls_and_indicator_render_in_both_languages_without_covering_icons() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let load = i18n::LanguageSnapshot::load(&assets, language, 1).unwrap();
        let _language = i18n::enter_snapshot(Arc::new(load.snapshot));
        for main in [Rect::new(2, 3, 108, 14), Rect::new(2, 3, 42, 9)] {
            for editing in [false, true] {
                let mut home = model(4);
                home.editing = editing;
                home.drop_target = Some(GridDropTarget {
                    item_index: 0,
                    side: GridDropSide::Before,
                });
                let layout = ui::home_layout(main, &home);
                let button = ui::home_edit_area(main, &home).unwrap();
                assert!(
                    layout
                        .items
                        .iter()
                        .all(|item| item.area.bottom() <= button.y)
                );
                assert!(button.right() <= main.right());
                let mut terminal =
                    Terminal::new(TestBackend::new(main.right(), main.bottom())).unwrap();
                let theme = ui::TundraTheme::default_dark();
                let context =
                    ui::RenderContext::from_theme(&theme, Default::default(), Default::default());
                terminal
                    .draw(|frame| ui::render_home_content(frame, main, &home, &context, None))
                    .unwrap();
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>();
                let label = if editing {
                    i18n::tr!("ui-home-edit-done")
                } else {
                    i18n::tr!("ui-home-edit")
                };
                let compact = |text: &str| {
                    text.chars()
                        .filter(|ch| !ch.is_whitespace())
                        .collect::<String>()
                };
                assert!(
                    compact(&text).contains(&compact(&label)),
                    "{language}: {text}"
                );
                let first = layout.items[0].area;
                assert_eq!(
                    terminal
                        .backend()
                        .buffer()
                        .cell((first.x, first.y))
                        .unwrap()
                        .symbol()
                        == "┃",
                    editing
                );
            }
        }
    }
    assert!(ui::home_edit_area(Rect::new(0, 0, 120, 40), &model(0)).is_none());
}

#[test]
fn home_icon_buttons_share_full_tile_geometry_with_pointer_targets() {
    let home = model(8);
    let main = Rect::new(3, 2, 108, 18);
    let layout = ui::home_layout(main, &home);
    let theme = ui::TundraTheme::default_dark();
    let mut context = ui::RenderContext::from_theme(&theme, Default::default(), Default::default());
    let buttons = ui::components::ButtonFrame::new(None, None, &theme);
    context.buttons = Some(buttons.clone());
    let mut terminal = Terminal::new(TestBackend::new(main.right(), main.bottom())).unwrap();
    terminal
        .draw(|frame| ui::render_home_content(frame, main, &home, &context, None))
        .unwrap();
    for tile in &layout.items {
        let icon = ui::home_entry_icon_area(tile.area);
        assert_eq!(
            (icon.x, icon.y, icon.width),
            (tile.area.x, tile.area.y, tile.area.width)
        );
        assert_eq!(
            layout.entry_at((tile.area.x, tile.area.y)),
            Some(tile.index)
        );
        let region = buttons
            .regions()
            .into_iter()
            .find(|region| region.id.as_str() == format!("home.entry.{}", tile.index))
            .unwrap();
        assert_eq!(region.area, tile.area);
    }
    assert_eq!(layout.items[1].area.x - layout.items[0].area.right(), 1);
    assert_eq!(layout.items[4].area.y - layout.items[0].area.bottom(), 1);
}
