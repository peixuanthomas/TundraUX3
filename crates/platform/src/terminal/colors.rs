//! Live palette queries inspired by Microsoft Edit's setup_terminal (MIT).
//! Reference: microsoft/edit 1be69c85a63bcd475a0b397196d0be3dbed60023.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalColors {
    pub palette: [Option<[u8; 3]>; 16],
    pub foreground: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    /// Some(true) only after a successful XTGETTCAP RGB reply with at least
    /// eight bits per channel. Unsupported
    /// queries are inconclusive, since the terminal may still support RGB.
    pub true_color: Option<bool>,
}

pub(super) fn supports_true_color(
    colors: TerminalColors,
    value: impl Fn(&str) -> Option<String>,
) -> bool {
    colors.true_color.unwrap_or_else(|| {
        value("COLORTERM").is_some_and(|value| {
            value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
        }) || value("TERM").is_some_and(|value| {
            let value = value.to_ascii_lowercase();
            value.contains("truecolor") || value.contains("direct")
        }) || value("WT_SESSION").is_some_and(|value| !value.is_empty())
    })
}

#[cfg(any(unix, windows, test))]
pub(super) fn parse_responses(raw: &[u8]) -> TerminalColors {
    let mut colors = TerminalColors::default();
    let mut position = 0;
    while position + 1 < raw.len() {
        if raw[position] != 0x1b || !matches!(raw[position + 1], b']' | b'P') {
            position += 1;
            continue;
        }
        let osc = raw[position + 1] == b']';
        let start = position + 2;
        let mut end = start;
        while end < raw.len() && !(osc && raw[end] == 7) && !raw[end..].starts_with(b"\x1b\\") {
            // Resynchronize a malformed string at a fresh report prefix.
            if raw[end..].starts_with(b"\x1b]") || raw[end..].starts_with(b"\x1bP") {
                break;
            }
            end += 1;
        }
        if end == raw.len() {
            break;
        }
        if raw[end..].starts_with(b"\x1b]") || raw[end..].starts_with(b"\x1bP") {
            position = end;
            continue;
        }
        if let Ok(body) = std::str::from_utf8(&raw[start..end]) {
            if osc {
                parse_osc(body, &mut colors);
            } else if body == "1+r524742" || body.starts_with("1+r524742=") {
                let value = body.strip_prefix("1+r524742=");
                if let Some(supported) = parse_rgb_capability(value) {
                    colors.true_color = Some(supported);
                }
            }
        }
        position = end + if raw[end] == 7 { 1 } else { 2 };
    }
    colors
}

#[cfg(any(unix, windows, test))]
fn parse_rgb_capability(value: Option<&str>) -> Option<bool> {
    // ncurses RGB may be boolean, bits per channel, or "r/g/b" bit counts.
    // https://invisible-island.net/ncurses/man/user_caps.5.html
    let Some(value) = value else {
        return Some(true);
    };
    if value.is_empty() || value.len() % 2 != 0 {
        return None;
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect::<Option<Vec<_>>>()?;
    let value = std::str::from_utf8(&bytes).ok()?;
    let bits = value
        .split('/')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if !matches!(bits.len(), 1 | 3) || bits.contains(&0) {
        return None;
    }
    Some(bits.iter().all(|bits| *bits >= 8))
}

#[cfg(any(unix, windows, test))]
fn parse_osc(body: &str, colors: &mut TerminalColors) {
    let mut parts = body.split(';');
    match parts.next() {
        Some("4") => {
            while let (Some(index), Some(value)) = (parts.next(), parts.next()) {
                if let (Ok(index @ 0..=15), Some(rgb)) = (index.parse::<usize>(), parse_rgb(value))
                {
                    colors.palette[index] = Some(rgb);
                }
            }
        }
        Some("10") => {
            if let Some(rgb) = parts.next().and_then(parse_rgb) {
                colors.foreground = Some(rgb);
            }
        }
        Some("11") => {
            if let Some(rgb) = parts.next().and_then(parse_rgb) {
                colors.background = Some(rgb);
            }
        }
        _ => {}
    }
}

#[cfg(any(unix, windows, test))]
fn parse_rgb(value: &str) -> Option<[u8; 3]> {
    let mut parts = value.strip_prefix("rgb:")?.split('/');
    let mut rgb = [0; 3];
    for channel in &mut rgb {
        let part = parts.next()?;
        if !(1..=4).contains(&part.len()) || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let value = u32::from_str_radix(part, 16).ok()?;
        let max = (1_u32 << (part.len() * 4)) - 1;
        *channel = ((value * 255 + max / 2) / max) as u8;
    }
    parts.next().is_none().then_some(rgb)
}

#[cfg(test)]
#[path = "../../tests/unit/terminal/colors_tests.rs"]
mod tests;
