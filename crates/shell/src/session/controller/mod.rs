mod account;
pub(super) mod auto_admin;
mod input;
pub(super) use auto_admin::{AutoAdminJob, AutoAdminState};
mod clock;
mod command_line;
mod diagnostics;
mod editor;
mod explorer;
mod launcher;
mod notifications;
mod settings;
pub(super) mod system_status;
pub(super) use account::tasks::*;
pub(super) use input::touch_pages::*;

pub(super) use diagnostics::*;
pub(super) use editor::tasks::*;
pub(super) use editor::*;
pub(super) use explorer::tasks::*;
pub(super) use input::hit_testing::*;
pub(super) use launcher::tasks::*;
pub(super) use settings::tasks::*;

mod logs;
pub(super) use logs::*;
mod management;
#[cfg(target_os = "linux")]
pub(in crate::session) mod privilege_session;
pub(super) use management::*;
