use ratatui::Terminal;
use ratatui::backend::TestBackend;
use tundra_ui::{
    ExplorerDialogViewModel, ExplorerEntryViewModel, ExplorerSearchViewModel, ExplorerViewModel,
    HomeDisplayMode, ShellChromeViewModel, StatusViewModel, TundraTheme,
    explorer_first_entry_content_line, render_explorer,
};

#[test]
fn explorer_renderer_shows_path_entries_details_search_and_message() {
    let model = ExplorerViewModel {
        current_path: "/Users/strix/projects".to_string(),
        entries: vec![
            ExplorerEntryViewModel {
                name: "src".to_string(),
                kind: "Directory".to_string(),
                size: None,
                modified: Some("2026-07-02 09:10".to_string()),
                attributes: vec!["hidden".to_string()],
                selected: false,
            },
            ExplorerEntryViewModel {
                name: "README.md".to_string(),
                kind: "File".to_string(),
                size: Some("1.2 KB".to_string()),
                modified: Some("2026-07-02 10:15".to_string()),
                attributes: vec!["readonly".to_string()],
                selected: true,
            },
        ],
        selected_index: Some(1),
        search: Some(ExplorerSearchViewModel::new("read", true, Some(1))),
        show_hidden: true,
        message: Some("Copied README.md".to_string()),
        error: None,
        pending_dialog: None,
    };

    let output = render_output(&model);

    assert!(output.contains("Path: /Users/strix/projects"));
    assert!(output.contains("Hidden files: shown"));
    assert!(output.contains("Search: read (1 match, active)"));
    assert!(output.contains("src | Directory"));
    assert!(output.contains("README.md | File | 1.2 KB"));
    assert!(output.contains("Selected: README.md"));
    assert!(output.contains("Name: README.md"));
    assert!(output.contains("Type: File"));
    assert!(output.contains("Size: 1.2 KB"));
    assert!(output.contains("Modified: 2026-07-02 10:15"));
    assert!(output.contains("Attributes: readonly"));
    assert!(output.contains("Copied README.md"));
    assert!(output.contains("Explorer"));
    assert!(output.contains("Ready"));
    assert!(output.contains("TundraUX 3"));
    assert!(output.contains("Enter: open"));
    assert!(output.contains("Backspace: parent"));
    assert!(output.contains("/: search"));
}

#[test]
fn explorer_renderer_shows_pending_confirmation_dialog() {
    let mut model = sample_model();
    model.pending_dialog = Some(ExplorerDialogViewModel::new(
        "Delete File",
        "Delete README.md?",
        "Enter: delete",
        "Esc: cancel",
    ));

    let output = render_output(&model);

    assert!(output.contains("Delete File"));
    assert!(output.contains("Delete README.md?"));
    assert!(output.contains("Enter: delete"));
    assert!(output.contains("Esc: cancel"));
}

#[test]
fn explorer_renderer_shows_error() {
    let mut model = sample_model();
    model.error = Some("Permission denied: README.md".to_string());

    let output = render_output(&model);

    assert!(output.contains("Error: Permission denied: README.md"));
}

#[test]
fn explorer_view_model_returns_selected_entry() {
    let model = sample_model();

    assert_eq!(
        model.selected_entry().map(|entry| entry.name.as_str()),
        Some("README.md")
    );
}

#[test]
fn explorer_first_entry_line_accounts_for_wrapped_header_text() {
    let model = sample_model();

    assert!(
        explorer_first_entry_content_line(&model, 40)
            > explorer_first_entry_content_line(&model, 120)
    );
}

fn sample_model() -> ExplorerViewModel {
    ExplorerViewModel {
        current_path: "/Users/strix/projects".to_string(),
        entries: vec![
            ExplorerEntryViewModel {
                name: "src".to_string(),
                kind: "Directory".to_string(),
                size: None,
                modified: None,
                attributes: Vec::new(),
                selected: false,
            },
            ExplorerEntryViewModel {
                name: "README.md".to_string(),
                kind: "File".to_string(),
                size: Some("1.2 KB".to_string()),
                modified: Some("2026-07-02 10:15".to_string()),
                attributes: vec!["readonly".to_string()],
                selected: true,
            },
        ],
        selected_index: Some(1),
        search: None,
        show_hidden: false,
        message: None,
        error: None,
        pending_dialog: None,
    }
}

fn chrome_for(screen: &str) -> ShellChromeViewModel {
    ShellChromeViewModel {
        app_name: "TundraUX 3".to_string(),
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (110, 32),
        screen_stack: vec![screen.to_string()],
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
        },
    }
}

fn render_output(model: &ExplorerViewModel) -> String {
    let chrome = chrome_for("Explorer");
    let mut terminal = Terminal::new(TestBackend::new(110, 32)).expect("test terminal");
    terminal
        .draw(|frame| {
            render_explorer(
                frame,
                frame.area(),
                &chrome,
                model,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render explorer");
    terminal_output(&terminal)
}

fn terminal_output(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
