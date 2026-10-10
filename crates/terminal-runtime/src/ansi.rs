use ratatui::style::Color;

/// Encodes a ratatui color as an ANSI foreground sequence. Named colors use
/// standard ANSI SGR codes; RGB and indexed colors retain their exact values.
pub fn ansi_foreground(color: Color) -> String {
    match color {
        Color::Reset => "\x1B[39m".to_string(),
        Color::Black => "\x1B[30m".to_string(),
        Color::Red => "\x1B[31m".to_string(),
        Color::Green => "\x1B[32m".to_string(),
        Color::Yellow => "\x1B[33m".to_string(),
        Color::Blue => "\x1B[34m".to_string(),
        Color::Magenta => "\x1B[35m".to_string(),
        Color::Cyan => "\x1B[36m".to_string(),
        Color::Gray => "\x1B[37m".to_string(),
        Color::DarkGray => "\x1B[90m".to_string(),
        Color::LightRed => "\x1B[91m".to_string(),
        Color::LightGreen => "\x1B[92m".to_string(),
        Color::LightYellow => "\x1B[93m".to_string(),
        Color::LightBlue => "\x1B[94m".to_string(),
        Color::LightMagenta => "\x1B[95m".to_string(),
        Color::LightCyan => "\x1B[96m".to_string(),
        Color::White => "\x1B[97m".to_string(),
        Color::Rgb(red, green, blue) => format!("\x1B[38;2;{red};{green};{blue}m"),
        Color::Indexed(index) => format!("\x1B[38;5;{index}m"),
    }
}
