use super::*;

fn query(protocol_type: ProtocolType, complete: bool, hinted: bool) -> TerminalCapabilityQuery {
    TerminalCapabilityQuery {
        protocol_type,
        font_size: DEFAULT_TERMINAL_FONT_SIZE,
        is_tmux: false,
        complete,
        had_unverified_graphics_hint: hinted,
        text_sizing_protocol: false,
    }
}

#[test]
fn capability_query_orders_confirmations_before_one_final_terminator() {
    for (compatibility, expects_standard_queries) in [
        (TerminalGraphicsCompatibility::Standard, true),
        (TerminalGraphicsCompatibility::WezTerm, false),
    ] {
        for is_tmux in [false, true] {
            let query = terminal_capability_query(is_tmux, compatibility);
            let (_, escape, end) = CapabilityParser::tmux_start_escape_end(is_tmux);
            let iterm = query.find("]1337;Capabilities").expect("iTerm query");
            let status = format!("{escape}[5n");
            let terminator = query.rfind(&status).expect("final status query");

            assert!(iterm < terminator);
            assert_eq!(query.matches(&status).count(), 1);
            assert!(query.ends_with(&format!("{status}{end}")));
            assert_eq!(query.contains("_Gi=31"), expects_standard_queries);
            assert_eq!(query.contains("[c"), expects_standard_queries);
            if expects_standard_queries {
                assert!(query.find("_Gi=31").unwrap() < query.find("[c").unwrap());
                assert!(query.find("[c").unwrap() < iterm);
            }
        }
    }
}

#[test]
fn wezterm_completed_handshake_falls_back_to_iterm2() {
    for is_tmux in [false, true] {
        let mut responses = TerminalResponseCollector::new();
        responses.push_bytes(b"\x1b[0n");
        let interpreted = interpret_terminal_capability_responses(
            responses,
            is_tmux,
            Some(DEFAULT_TERMINAL_FONT_SIZE),
            TerminalGraphicsCompatibility::WezTerm,
        );
        assert_eq!(interpreted.protocol_type, ProtocolType::Iterm2);
    }
}

#[test]
fn protocol_priority_and_text_sizing_match_live_response_semantics() {
    let raw = b"\x1b[?64;4c\x1b[1;1R\x1b_Gi=31;OK\x1b\\\x1b[1;3R\x1b[2;5R\x1b[0n";
    let mut parser = CapabilityParser::new();
    let responses = raw
        .iter()
        .flat_map(|byte| parser.push(char::from(*byte)))
        .collect::<Vec<_>>();
    assert_eq!(
        standard_protocol_from_responses(&responses),
        Some(ProtocolType::Kitty)
    );
    assert_eq!(
        standard_protocol_from_responses(&[CapabilityResponse::Sixel]),
        Some(ProtocolType::Sixel)
    );
    assert!(responses_support_text_sizing_protocol(&responses));
    assert!(!responses_support_text_sizing_protocol(&[
        CapabilityResponse::CursorPositionReport(1, 1),
        CapabilityResponse::CursorPositionReport(3, 1),
        CapabilityResponse::CursorPositionReport(4, 1),
    ]));
}

#[test]
fn cell_geometry_priority_is_response_then_native_then_default() {
    let interpret = |responses, native| {
        interpret_terminal_capability_responses(
            TerminalResponseCollector {
                parser: CapabilityParser::new(),
                responses,
                raw: Vec::new(),
                complete: true,
            },
            false,
            native,
            TerminalGraphicsCompatibility::Standard,
        )
        .font_size
    };
    let response = interpret(
        vec![CapabilityResponse::CellSize(Some((7, 13)))],
        Some(FontSize::new(8, 16)),
    );
    assert_eq!((response.width, response.height), (7, 13));
    let native = interpret(Vec::new(), Some(FontSize::new(8, 16)));
    assert_eq!((native.width, native.height), (8, 16));
    let invalid_response = interpret(
        vec![CapabilityResponse::CellSize(Some((0, 13)))],
        Some(FontSize::new(9, 18)),
    );
    assert_eq!((invalid_response.width, invalid_response.height), (9, 18));
    for invalid_native in [FontSize::new(0, 16), FontSize::new(8, 0)] {
        let fallback = interpret(Vec::new(), Some(invalid_native));
        assert_eq!(
            (fallback.width, fallback.height),
            (
                DEFAULT_TERMINAL_FONT_SIZE.width,
                DEFAULT_TERMINAL_FONT_SIZE.height
            )
        );
    }
    for geometry in [
        (0, 320, 80, 20),
        (640, 0, 80, 20),
        (79, 320, 80, 20),
        (640, 19, 80, 20),
    ] {
        assert!(
            font_size_from_terminal_geometry(geometry.0, geometry.1, geometry.2, geometry.3)
                .is_none()
        );
    }
}

#[test]
fn deadline_wait_never_waits_after_deadline() {
    let now = Instant::now();
    assert_eq!(deadline_wait_millis(now, now), None);
    assert_eq!(
        deadline_wait_millis(now + Duration::from_nanos(1), now),
        Some(1)
    );
    assert_eq!(
        deadline_wait_millis(now + Duration::from_millis(25), now),
        Some(25)
    );
    assert_eq!(
        deadline_wait_millis(now, now + Duration::from_millis(1)),
        None
    );
}

#[test]
fn probe_status_preserves_verified_unsupported_and_no_response() {
    assert_eq!(
        terminal_probe_from_query(query(ProtocolType::Kitty, false, false)).status,
        TerminalGraphicsProbeStatus::Verified(TerminalGraphicsProtocol::Kitty)
    );
    assert_eq!(
        terminal_probe_from_query(query(ProtocolType::Halfblocks, true, false)).status,
        TerminalGraphicsProbeStatus::Unsupported
    );
    assert!(matches!(
        terminal_probe_from_query(query(ProtocolType::Halfblocks, false, false)).status,
        TerminalGraphicsProbeStatus::NoResponse { .. }
    ));
    assert!(matches!(
        terminal_probe_from_query(query(ProtocolType::Halfblocks, true, true)).status,
        TerminalGraphicsProbeStatus::NoResponse { .. }
    ));
}

#[test]
fn response_collection_stops_at_status_and_preserves_geometry() {
    let mut input = b"\x1b[6;9;17t\x1b_Gi=31;OK\x1b\\\x1b[0nx".iter().copied();
    let mut responses = TerminalResponseCollector::new();
    while !responses.complete {
        responses.push_byte(input.next().expect("complete response"));
    }
    assert_eq!(input.next(), Some(b'x'));
    let capabilities = terminal_probe_from_query(interpret_terminal_capability_responses(
        responses,
        false,
        None,
        TerminalGraphicsCompatibility::Standard,
    ));
    assert_eq!(
        capabilities.cell_size,
        Some(TerminalCellSize {
            width: 17,
            height: 9
        })
    );
}

#[test]
fn environment_compatibility_is_injected_without_process_mutation() {
    let values = |name: &str| match name {
        "TERM_PROGRAM" => Some("WezTerm".to_string()),
        _ => None,
    };
    assert_eq!(
        terminal_graphics_compatibility_with(values),
        TerminalGraphicsCompatibility::WezTerm
    );
    assert!(terminal_has_graphics_environment_hint_with(false, |name| {
        (name == "LC_TERMINAL").then(|| "iTerm2".to_string())
    }));
    assert!(terminal_has_graphics_environment_hint_with(true, |name| {
        (name == "ITERM_SESSION_ID").then(|| "session".to_string())
    }));
}

#[test]
fn response_parsers_cover_iterm_text_sizing_and_native_geometry() {
    assert_eq!(
        parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=AFN\x1b\\"),
        Some(Iterm2GraphicsCapabilities {
            file: true,
            sixel: false
        })
    );
    assert_eq!(
        parse_iterm2_graphics_capabilities(b"prefix\x1b]1337;Capabilities=ASxN\x07suffix"),
        Some(Iterm2GraphicsCapabilities {
            file: false,
            sixel: true
        })
    );
    assert_eq!(
        parse_iterm2_graphics_capabilities(b"\x1b]1337;Capabilities=FooN\x07"),
        Some(Iterm2GraphicsCapabilities {
            file: false,
            sixel: false
        })
    );
    assert_eq!(parse_iterm2_graphics_capabilities(b"\x1b[?1;0c"), None);
    assert!(responses_support_text_sizing_protocol(&[
        CapabilityResponse::CursorPositionReport(1, 1),
        CapabilityResponse::CursorPositionReport(3, 1),
        CapabilityResponse::CursorPositionReport(5, 1),
    ]));
    let size = font_size_from_terminal_geometry(640, 320, 80, 20).unwrap();
    assert_eq!((size.width, size.height), (8, 16));
}

#[test]
fn windows_console_record_mapping_only_accepts_key_down_bytes() {
    assert_eq!(windows_vt_byte_from_record(false, true, b'x'.into()), None);
    assert_eq!(windows_vt_byte_from_record(true, false, b'x'.into()), None);
    assert_eq!(
        windows_vt_byte_from_record(true, true, b'x'.into()),
        Some(b'x')
    );
    assert_eq!(windows_vt_byte_from_record(true, true, 0x100), None);
}

#[test]
fn ui_source_and_manifest_do_not_own_terminal_probing() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ui");
    fn rust_source(directory: &std::path::Path, output: &mut String) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_source(&path, output);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                output.push_str(&std::fs::read_to_string(path).unwrap());
            }
        }
    }
    let mut source = String::new();
    rust_source(&root.join("src"), &mut source);
    for forbidden in [
        "std::env",
        "stdin()",
        "stdout()",
        "libc::",
        "GetStdHandle",
        "prepare_path",
        "ImageReader::open",
    ] {
        assert!(
            !source.contains(forbidden),
            "UI retained forbidden boundary: {forbidden}"
        );
    }
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("libc ="));
    assert!(!manifest.contains("windows-sys"));
    assert!(!manifest.contains("platform ="));
    assert!(!manifest.contains("system-services ="));
    let platform_source = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/terminal.rs"),
    )
    .unwrap()
    .replace("\r\n", "\n");
    assert!(platform_source.contains("probe_terminal_graphics_capabilities"));
    assert!(platform_source.contains("CapabilityParser"));
    for declaration in [
        "const TERMINAL_CAPABILITY_QUERY_TIMEOUT: Duration",
        "const DEFAULT_TERMINAL_FONT_SIZE: FontSize",
    ] {
        let declaration = platform_source.find(declaration).unwrap();
        let prefix = &platform_source[..declaration];
        assert!(
            prefix.ends_with("#[cfg(any(unix, windows))]\n"),
            "target-specific declaration lost its matching cfg: {declaration}"
        );
    }
}
