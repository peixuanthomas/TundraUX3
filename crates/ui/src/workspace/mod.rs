mod frame;
mod layout;
mod panel;

pub(crate) use frame::{FrameColors, capture_color};
pub use frame::{capture_workspace_controls, capture_workspace_frame, render_workspace_frame};
pub use layout::{WorkspaceLayout, WorkspacePaneLayout, WorkspaceSplitMode};
pub use panel::{
    WorkspaceAction, WorkspaceCloseWait, WorkspacePanePlaceholder, WorkspacePanel,
    WorkspacePanelViewModel, WorkspaceWindowViewModel, render_workspace_chrome_button,
    render_workspace_close_wait, render_workspace_pane_placeholder, render_workspace_pane_title,
    render_workspace_panel, render_workspace_resize_notice,
};
