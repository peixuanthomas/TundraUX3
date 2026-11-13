use super::*;
use crate::components::{Button, DialogLayout, ProgressGauge};

pub fn editor_vim_install_layout(area: Rect) -> (Rect, Rect) {
    let width = area.width.min(68);
    let height = area.height.min(10);
    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let footer = DialogLayout::new(dialog, 1, None).footer;
    let cancel = crate::right_aligned_actions(
        footer,
        &[crate::components::terminal_width(&i18n::tr!("vim-install-cancel")) as u16 + 2],
    )[0];
    (dialog, cancel)
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &EditorVimInstallViewModel,
    context: &crate::RenderContext,
) {
    use app::update::VimInstallPhase::*;
    let (dialog, cancel) = editor_vim_install_layout(area);
    let chrome = DialogLayout::new(dialog, 1, None);
    chrome.render_frame(frame, &i18n::tr!("vim-install-title"), context);
    let status = if model.cancelling {
        i18n::tr!("vim-install-cancelling")
    } else {
        i18n::tr!(match model.progress.phase {
            Checking => "vim-install-checking",
            Downloading => "vim-install-downloading",
            Extracting => "vim-install-extracting",
            Verifying => "vim-install-verifying",
        })
    };
    frame.render_widget(
        Paragraph::new(status)
            .style(Style::default().fg(context.theme.text))
            .wrap(ratatui::widgets::Wrap { trim: true }),
        Rect::new(
            chrome.body.x,
            chrome.body.y,
            chrome.body.width,
            chrome.body.height.saturating_sub(1).min(2),
        ),
    );
    let progress = model.progress;
    let ratio = if progress.total == 0 {
        0.0
    } else {
        progress.downloaded as f64 / progress.total as f64
    };
    let label = i18n::tr!(
        "vim-install-percent",
        percent = (ratio * 100.0).clamp(0.0, 100.0) as u32
    );
    frame.render_widget(
        ProgressGauge::new(
            label,
            ratio,
            context.theme.accent,
            context.theme.surface,
            &context.theme,
        ),
        Rect::new(
            chrome.body.x,
            chrome.body.bottom().saturating_sub(1).max(chrome.body.y),
            chrome.body.width,
            chrome.body.height.min(1),
        ),
    );
    let mut button = Button::new("editor.vim-install.cancel", i18n::tr!("vim-install-cancel"));
    button.state.focused = true;
    button.state.disabled = model.cancelling;
    button.render_inline_frame(frame, cancel, &DialogLayout::footer_theme(context));
}
