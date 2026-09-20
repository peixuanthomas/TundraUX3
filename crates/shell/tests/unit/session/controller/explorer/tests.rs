use super::{explorer_copy_modifier, explorer_toggle_modifier};
use crate::InputModifiers;
use platform::PlatformKind;

#[test]
fn linux_explorer_selection_modifiers_use_control() {
    assert!(explorer_toggle_modifier(
        PlatformKind::Linux,
        InputModifiers::CTRL
    ));
    assert!(explorer_copy_modifier(
        PlatformKind::Linux,
        InputModifiers::CTRL
    ));
    assert!(!explorer_toggle_modifier(
        PlatformKind::Linux,
        InputModifiers::ALT
    ));
    assert!(!explorer_copy_modifier(
        PlatformKind::Linux,
        InputModifiers::ALT
    ));
}

#[cfg(unix)]
#[test]
fn quick_location_helpers_preserve_raw_non_utf8_paths() {
    use super::{explorer_quick_location_command, explorer_quick_location_index};
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let raw_path =
        std::path::PathBuf::from("/tmp").join(OsString::from_vec(b"raw-volume-\xff".to_vec()));
    let mut state = app::explorer::ExplorerState::new("/current", false);
    state.quick_locations = vec![
        app::explorer::ExplorerQuickLocation::new("before", "Before", "/before", "folder"),
        app::explorer::ExplorerQuickLocation::volume("raw", "Raw", raw_path.clone()),
        app::explorer::ExplorerQuickLocation::new("after", "After", "/after", "folder"),
    ];
    let mut model = ui::ExplorerViewModel::new("/current", Vec::new(), None);
    model.quick_locations = state
        .quick_locations
        .iter()
        .map(|location| {
            let mut view = ui::ExplorerQuickLocationViewModel::new(
                location.id.clone(),
                location.localized_label().render_current(),
                location.path.display().to_string(),
                location.icon_key.clone(),
            );
            view.kind = location.kind;
            view
        })
        .collect();
    model.quick_locations[1].current = true;

    assert_eq!(explorer_quick_location_index(&model, true), Some(2));
    assert_eq!(explorer_quick_location_index(&model, false), Some(0));
    assert_eq!(
        explorer_quick_location_command(&model, &state, 1),
        Some(app::explorer::ExplorerCommand::Navigate(raw_path))
    );
}
