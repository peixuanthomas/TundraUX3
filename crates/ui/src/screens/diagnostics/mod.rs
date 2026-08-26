mod layout;
mod model;
mod render;

pub use layout::{
    DiagnosticsContentLayout, DiagnosticsHitTarget, DiagnosticsLayout,
    DiagnosticsRepairDialogLayout, DiagnosticsRowLayout, DiagnosticsScrollbarLayout,
    DiagnosticsTabLayout, diagnostics_content_hit_test, diagnostics_content_layout,
    diagnostics_hit_test, diagnostics_layout, diagnostics_repair_dialog_hit_test,
    diagnostics_repair_dialog_layout,
};
pub use model::{
    DebugDiagnosticsViewModel, DiagnosticsCheckViewModel, DiagnosticsIncidentViewModel,
    DiagnosticsLogViewModel, DiagnosticsRepairDialogViewModel, DiagnosticsRepairItemViewModel,
    DiagnosticsStatus, DiagnosticsTab, DiagnosticsViewModel,
};
pub use render::{render_diagnostics, render_diagnostics_contextual};
pub(crate) use render::{
    render_diagnostics_content, render_diagnostics_footer, render_diagnostics_header,
    render_diagnostics_repair_dialog,
};
