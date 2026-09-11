use ratatui::style::Color;

pub use app::{setup_language_options, setup_timezone_options};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupColorOption {
    pub label: String,
    pub value: &'static str,
    pub color: Color,
}

pub fn setup_standard_color_options() -> Vec<SetupColorOption> {
    vec![
        SetupColorOption {
            label: i18n::tr!("ui-auth-color-white"),
            value: "white",
            color: Color::White,
        },
        SetupColorOption {
            label: i18n::tr!("ui-auth-color-cyan"),
            value: "cyan",
            color: Color::Cyan,
        },
        SetupColorOption {
            label: i18n::tr!("ui-auth-color-blue"),
            value: "blue",
            color: Color::Blue,
        },
        SetupColorOption {
            label: i18n::tr!("ui-auth-color-green"),
            value: "green",
            color: Color::Green,
        },
        SetupColorOption {
            label: i18n::tr!("ui-auth-color-yellow"),
            value: "yellow",
            color: Color::Yellow,
        },
        SetupColorOption {
            label: i18n::tr!("ui-auth-color-magenta"),
            value: "magenta",
            color: Color::Magenta,
        },
        SetupColorOption {
            label: i18n::tr!("ui-auth-color-red"),
            value: "red",
            color: Color::Red,
        },
    ]
}
