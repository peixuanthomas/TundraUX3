use crate::RuntimeAsciiAssets;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClockCreateDialogFocus {
    #[default]
    Input,
    CreateAlarm,
    CreateCountdown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockEntryViewModel {
    pub id: u64,
    pub label: String,
    pub strong: bool,
}

impl ClockEntryViewModel {
    pub fn new(id: u64, label: impl Into<String>, strong: bool) -> Self {
        Self {
            id,
            label: label.into(),
            strong,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockCreateDialogViewModel {
    pub input: String,
    pub error: Option<String>,
    pub focus: ClockCreateDialogFocus,
}

impl ClockCreateDialogViewModel {
    pub fn new(input: impl Into<String>) -> Self {
        Self {
            input: input.into(),
            error: None,
            focus: ClockCreateDialogFocus::Input,
        }
    }
}

impl Default for ClockCreateDialogViewModel {
    fn default() -> Self {
        Self::new("")
    }
}

/// Physical height-to-width ratio of one terminal character cell.
///
/// Terminals may omit pixel dimensions, so the conventional 2:1 character
/// cell is used as a fallback. Keeping this value explicit lets circular
/// graphics compensate for fonts and line heights which use another ratio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalCellAspectRatio(f64);

impl TerminalCellAspectRatio {
    pub const FALLBACK: Self = Self(2.0);

    pub fn new(height_to_width: f64) -> Option<Self> {
        (height_to_width.is_finite() && height_to_width > 0.0).then_some(Self(height_to_width))
    }

    /// Derives the average cell ratio from terminal character and pixel sizes.
    ///
    /// Crossterm reports zero pixel dimensions on terminals which do not
    /// support them; those and all other invalid dimensions use the fallback.
    pub fn from_window_size(columns: u16, rows: u16, pixel_width: u16, pixel_height: u16) -> Self {
        if columns == 0 || rows == 0 || pixel_width == 0 || pixel_height == 0 {
            return Self::FALLBACK;
        }

        let height_to_width = (f64::from(pixel_height) * f64::from(columns))
            / (f64::from(pixel_width) * f64::from(rows));
        Self::new(height_to_width).unwrap_or(Self::FALLBACK)
    }

    pub fn height_to_width(self) -> f64 {
        self.0
    }
}

impl Eq for TerminalCellAspectRatio {}

impl Default for TerminalCellAspectRatio {
    fn default() -> Self {
        Self::FALLBACK
    }
}

#[derive(Debug, Clone)]
pub struct ClockViewModel {
    pub date: String,
    pub digital_time: String,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub alarms: Vec<ClockEntryViewModel>,
    pub countdowns: Vec<ClockEntryViewModel>,
    pub selected_entry_id: Option<u64>,
    /// Offset into the flattened `alarms` then `countdowns` display order.
    pub entry_window_start: usize,
    pub create_dialog: Option<ClockCreateDialogViewModel>,
    read_only: bool,
    ascii_assets: Option<RuntimeAsciiAssets>,
    terminal_cell_aspect_ratio: TerminalCellAspectRatio,
}

impl PartialEq for ClockViewModel {
    fn eq(&self, other: &Self) -> bool {
        self.date == other.date
            && self.digital_time == other.digital_time
            && self.hour == other.hour
            && self.minute == other.minute
            && self.second == other.second
            && self.alarms == other.alarms
            && self.countdowns == other.countdowns
            && self.selected_entry_id == other.selected_entry_id
            && self.entry_window_start == other.entry_window_start
            && self.create_dialog == other.create_dialog
            && self.read_only == other.read_only
            && self.terminal_cell_aspect_ratio == other.terminal_cell_aspect_ratio
    }
}

impl Eq for ClockViewModel {}

impl ClockViewModel {
    /// Compatibility constructor for callers which only have a formatted time label.
    /// New code should prefer [`ClockViewModel::at`].
    pub fn new(current_time: impl Into<String>) -> Self {
        let current_time = current_time.into();
        let mut date = String::new();
        let mut digital_time = current_time.clone();

        for part in current_time.split_whitespace() {
            if date.is_empty() && part.contains('-') {
                date = part.to_string();
            }
            if part.contains(':') {
                digital_time = part.to_string();
                break;
            }
        }

        let mut time_parts = digital_time
            .split(':')
            .filter_map(|part| part.parse::<u8>().ok());
        let hour = time_parts.next().unwrap_or(0);
        let minute = time_parts.next().unwrap_or(0);
        let second = time_parts.next().unwrap_or(0);

        Self::at(date, digital_time, hour, minute, second)
    }

    pub fn at(
        date: impl Into<String>,
        digital_time: impl Into<String>,
        hour: u8,
        minute: u8,
        second: u8,
    ) -> Self {
        Self {
            date: date.into(),
            digital_time: digital_time.into(),
            hour,
            minute,
            second,
            alarms: Vec::new(),
            countdowns: Vec::new(),
            selected_entry_id: None,
            entry_window_start: 0,
            create_dialog: None,
            read_only: false,
            ascii_assets: None,
            terminal_cell_aspect_ratio: TerminalCellAspectRatio::default(),
        }
    }

    /// Marks the Clock page as view-only.
    ///
    /// Read-only pages omit all controls which create clock entries. Input
    /// routing should use the zero-sized `new_button` returned by
    /// `clock_page_layout` as the matching hit-test contract.
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn with_ascii_assets(mut self, ascii_assets: RuntimeAsciiAssets) -> Self {
        self.ascii_assets = Some(ascii_assets);
        self
    }

    pub fn with_terminal_cell_aspect_ratio(
        mut self,
        terminal_cell_aspect_ratio: TerminalCellAspectRatio,
    ) -> Self {
        self.terminal_cell_aspect_ratio = terminal_cell_aspect_ratio;
        self
    }

    pub(crate) fn terminal_cell_aspect_ratio(&self) -> TerminalCellAspectRatio {
        self.terminal_cell_aspect_ratio
    }

    pub(crate) fn clock_font(&self) -> Option<&crate::ClockFontAsset> {
        self.ascii_assets
            .as_ref()
            .map(RuntimeAsciiAssets::clock_font)
    }
}

impl Default for ClockViewModel {
    fn default() -> Self {
        Self::at("", "00:00:00", 0, 0, 0)
    }
}
