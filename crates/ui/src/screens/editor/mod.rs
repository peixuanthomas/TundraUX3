use std::borrow::Cow;
use std::cmp::{max, min};
use std::collections::BTreeMap;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::TundraTheme;

pub use app::editor::EditorMode;
pub use app::rich_document::{NodeId, RichPosition, RichRange};

mod document;
mod layout;
mod model;
mod render;
mod source;

pub use layout::*;
pub use model::*;
pub use render::*;
pub use source::*;

#[cfg(test)]
mod tests;
