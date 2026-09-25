//! Context-aware shell modal renderers. Page content is dispatched through ScreenContent.

use ratatui::{Frame, layout::Rect};

use crate::*;

pub fn render_notification_overlay_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &NotificationViewModel,
    context: &RenderContext,
) {
    super::notifications::render_notification_overlay_context(frame, area, model, context);
}

pub fn render_exit_confirmation_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    context: &RenderContext,
) {
    render_exit_confirmation_contextual(frame, area, model, context);
}

pub fn render_time_sync_failure_dialog_with_context(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    context: &RenderContext,
) {
    render_time_sync_failure_dialog_contextual(frame, area, model, context);
}
