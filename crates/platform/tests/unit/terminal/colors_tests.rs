use super::*;

#[test]
fn color_depth_prefers_live_confirmation_then_environment_fallbacks() {
    let confirmed = TerminalColors {
        true_color: Some(true),
        ..Default::default()
    };
    assert!(supports_true_color(confirmed, |_| None));
    assert!(!supports_true_color(TerminalColors::default(), |_| None));
    assert!(!supports_true_color(
        TerminalColors {
            true_color: Some(false),
            ..Default::default()
        },
        |_| Some("truecolor".into())
    ));
    for (name, value) in [
        ("COLORTERM", "TrueColor"),
        ("COLORTERM", "24BIT"),
        ("TERM", "xterm-direct"),
        ("WT_SESSION", "session"),
    ] {
        assert!(supports_true_color(TerminalColors::default(), |key| (key
            == name)
            .then(|| value.into())));
    }
    assert!(!supports_true_color(TerminalColors::default(), |key| (key
        == "TERM")
        .then(|| "xterm-256color".into())));
}

#[test]
fn palette_and_defaults_accept_bel_st_and_scaled_channels() {
    let colors = parse_responses(b"\x1b]4;1;rgb:1234/abcd/ffff;8;rgb:0/8/f\x07\x1b]10;rgb:aa/bb/cc\x1b\\\x1b]11;rgb:000/800/fff\x07");
    assert_eq!(colors.palette[1], Some([18, 171, 255]));
    assert_eq!(colors.palette[8], Some([0, 136, 255]));
    assert_eq!(colors.foreground, Some([170, 187, 204]));
    assert_eq!(colors.background, Some([0, 128, 255]));
    assert_eq!(
        colors.true_color, None,
        "OSC replies do not prove RGB support"
    );
}

#[test]
fn malformed_partial_and_duplicate_reports_do_not_erase_valid_colors() {
    let colors = parse_responses(b"\x1b]4;1;rgb:ff/00/00\x07\x1b]4;1;rgb:gg/00/00;16;rgb:aa/bb/cc;2;rgb:ff/ff\x07\x1b]10;rgb:12345/00/00\x07\x1b]11;rgb:00/00/00/ff\x07\x1b]4;1;rgb:00/ff/00\x1b\\\x1b]10;rgb:ff/ff/ff");
    assert_eq!(colors.palette[1], Some([0, 255, 0]));
    assert_eq!(colors.palette[2], None);
    assert_eq!(colors.foreground, None);
    assert_eq!(colors.background, None);
}

#[test]
fn rgb_capability_requires_a_successful_well_formed_reply() {
    for body in [
        "1+r524742",
        "1+r524742=38",
        "1+r524742=3234",
        "1+r524742=382f382f38",
    ] {
        let response = format!("\x1bP{body}\x1b\\");
        assert_eq!(parse_responses(response.as_bytes()).true_color, Some(true));
    }
    for body in ["1+r524742=31", "1+r524742=352f362f35"] {
        let response = format!("\x1bP{body}\x1b\\");
        assert_eq!(parse_responses(response.as_bytes()).true_color, Some(false));
    }
    for body in [
        "0+r524742",
        "1+r524742=",
        "1+r524742=30",
        "1+r524742=zz",
        "1+r524742=3",
        "1+r436f=323536",
    ] {
        let response = format!("\x1bP{body}\x1b\\");
        assert_eq!(parse_responses(response.as_bytes()).true_color, None);
    }
}
