use crate::screens::{PageHelpScreen, page_help, page_help_notification};
use ratatui::layout::Rect;

#[test]
fn every_screen_has_actions_and_advanced_keys() {
    for screen in PageHelpScreen::ALL {
        let help = page_help(screen);
        assert!(!help.title.is_empty(), "{screen:?} title");
        assert!(help.sections.len() >= 2, "{screen:?} sections");
        assert!(
            help.sections.iter().all(|section| !section.rows.is_empty()),
            "{screen:?} rows"
        );
    }
}

#[test]
fn help_notification_has_no_tone_prefix_and_left_alignment() {
    let help = page_help(PageHelpScreen::Explorer);
    let notification = page_help_notification(&help, 0);
    assert!(!notification.show_tone_prefix);
    assert!(notification.align_left);
    assert_eq!(notification.actions.len(), 1);
    assert_eq!(notification.actions[0].label, help.close_label);
}

#[test]
fn help_fits_a_standard_terminal_with_scroll() {
    let help = page_help(PageHelpScreen::Explorer);
    let notification = page_help_notification(&help, 0);
    let layout = crate::notification_layout(Rect::new(0, 0, 80, 24), &notification);
    assert!(matches!(layout, crate::NotificationLayout::Dialog(_)));
}
