use ratatui::style::Color;

pub use app::{setup_language_options, setup_timezone_options};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupColorOption {
    pub label: &'static str,
    pub value: &'static str,
    pub color: Color,
}

const SETUP_STANDARD_COLORS: [SetupColorOption; 7] = [
    SetupColorOption {
        label: "White",
        value: "white",
        color: Color::White,
    },
    SetupColorOption {
        label: "Cyan",
        value: "cyan",
        color: Color::Cyan,
    },
    SetupColorOption {
        label: "Blue",
        value: "blue",
        color: Color::Blue,
    },
    SetupColorOption {
        label: "Green",
        value: "green",
        color: Color::Green,
    },
    SetupColorOption {
        label: "Yellow",
        value: "yellow",
        color: Color::Yellow,
    },
    SetupColorOption {
        label: "Magenta",
        value: "magenta",
        color: Color::Magenta,
    },
    SetupColorOption {
        label: "Red",
        value: "red",
        color: Color::Red,
    },
];

pub const fn setup_standard_color_options() -> &'static [SetupColorOption] {
    &SETUP_STANDARD_COLORS
}
