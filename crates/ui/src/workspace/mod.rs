mod chrome;
mod frame;
mod layout;
mod panel;
mod split_menu;
mod taskbar;

pub use chrome::{
    WorkspaceWindowAction, WorkspaceWindowButtonsState, WorkspaceWindowControls,
    render_workspace_pane_frame,
};
pub(crate) use frame::{FrameColors, capture_color};
pub use frame::{capture_workspace_controls, capture_workspace_frame, render_workspace_frame};
pub use layout::{WorkspaceLayout, WorkspacePaneLayout, WorkspaceSplitMode};
pub use panel::{
    WorkspaceAction, WorkspaceCloseWait, WorkspacePanePlaceholder, WorkspacePanel,
    WorkspacePanelViewModel, WorkspaceWindowViewModel, render_workspace_close_wait,
    render_workspace_pane_placeholder, render_workspace_panel, render_workspace_resize_notice,
};
pub use split_menu::{
    WorkspaceSplitMenu, WorkspaceSplitMenuViewModel, render_workspace_split_menu,
};
pub use taskbar::{
    WorkspaceTaskbar, WorkspaceTaskbarAction, WorkspaceTaskbarItem, render_workspace_taskbar,
};
