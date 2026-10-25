use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VimCursor {
    pub line: usize,
    /// Zero-based UTF-8 byte column, as used by Vim's col().
    pub column: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    #[default]
    Unix,
    Dos,
    Mac,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferInput {
    pub host_id: u64,
    pub path: Option<PathBuf>,
    pub text: String,
    pub encoding: String,
    pub line_ending: LineEnding,
    pub bom: bool,
    pub end_of_line: bool,
    pub modified: bool,
    pub cursor: VimCursor,
    pub read_only: bool,
    pub protected: bool,
}

impl BufferInput {
    pub fn new(host_id: u64, text: impl Into<String>) -> Self {
        let text = text.into();
        let end_of_line = text.ends_with('\n');
        Self {
            host_id,
            path: None,
            text,
            encoding: "utf-8".into(),
            line_ending: LineEnding::Unix,
            bom: false,
            end_of_line,
            modified: false,
            cursor: VimCursor::default(),
            read_only: false,
            protected: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VimLineChange {
    pub start_line: usize,
    pub end_line: usize,
    pub added: i64,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VimBuffer {
    pub number: u32,
    pub host_id: Option<u64>,
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub text: Arc<str>,
    #[serde(default)]
    pub text_changed: bool,
    #[serde(default)]
    pub line_changes: Vec<VimLineChange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_lines: Option<Vec<String>>,
    pub encoding: String,
    pub line_ending: LineEnding,
    pub bom: bool,
    pub end_of_line: bool,
    pub modified: bool,
    pub cursor: VimCursor,
    pub read_only: bool,
    pub protected: bool,
    pub changed_tick: i64,
}

#[derive(Debug, Clone, Default)]
pub struct VimState {
    pub ready: bool,
    pub revision: u64,
    pub last_command: u64,
    pub active_buffer: u32,
    pub mode: String,
    pub buffers: Vec<VimBuffer>,
    pub removed_buffers: Vec<u32>,
    pub exiting: bool,
    /// A successful Vim exit with dirty buffers intentionally discards them.
    pub discard_modified_on_exit: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StateRecord {
    pub ready: bool,
    pub revision: u64,
    pub last_command: u64,
    pub active_buffer: u32,
    pub mode: String,
    pub buffers: Vec<VimBuffer>,
    pub removed_buffers: Vec<u32>,
    pub exiting: bool,
    pub discard_modified_on_exit: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveRequest {
    pub id: u64,
    pub buffer_number: u32,
    pub host_id: Option<u64>,
    pub path: Option<PathBuf>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_lines: Option<Vec<String>>,
    /// Vim-encoded bytes, including its selected file format and BOM.
    pub bytes: Vec<u8>,
    pub encoding: String,
    pub line_ending: LineEnding,
    pub bom: bool,
    pub end_of_line: bool,
    pub changed_tick: i64,
    pub protected: bool,
    pub current_path: Option<PathBuf>,
    pub append: bool,
    pub whole_buffer: bool,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveResult {
    pub path: PathBuf,
}
