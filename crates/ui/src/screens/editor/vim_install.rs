use super::*;
use crate::components::{Button, ProgressGauge, Surface};

pub fn editor_vim_install_layout(area: Rect) -> (Rect, Rect) {
    let width = area.width.min(68);
    let height = area.height.min(10);
    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let button_width = (crate::components::terminal_width(&i18n::tr!("vim-install-cancel")) as u16
        + 4)
    .min(width.saturating_sub(4));
    let cancel = Rect::new(
        dialog.x + width.saturating_sub(button_width) / 2,
        dialog.y + height.saturating_sub(4),
        button_width,
        3.min(height),
    );
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
    frame.render_widget(Clear, dialog);
    Surface::new()
        .titled(i18n::tr!("vim-install-title"))
        .bordered(true)
        .raised(true)
        .render_frame(frame, dialog, context);
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
            dialog.x + 2,
            dialog.y + 2,
            dialog.width.saturating_sub(4),
            2,
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
            dialog.x + 2,
            dialog.y + 4,
            dialog.width.saturating_sub(4),
            1,
        ),
    );
    let mut button = Button::new("editor.vim-install.cancel", i18n::tr!("vim-install-cancel"));
    button.state.focused = true;
    button.state.disabled = model.cancelling;
    button.render_frame(frame, cancel, &context.compatibility_theme());
}
