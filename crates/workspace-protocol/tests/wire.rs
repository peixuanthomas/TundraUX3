use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use workspace_protocol::transport::Framed;
use workspace_protocol::{
    AppToShell, Color, Cursor, CursorShape, Heading, Hello, Image, LayoutGeneration,
    MIN_CONTENT_SIZE, ProcessGeneration, Rect, RunId, ServiceError, ServiceReply, SessionId, Size,
    WindowFrame, WorkerId, WorkerRole, WorkerToSupervisor,
};

#[test]
fn handshake_preserves_full_run_and_worker_identity() {
    let run = RunId(u128::MAX);
    let worker = WorkerId {
        run,
        role: WorkerRole::App(SessionId(17)),
        generation: ProcessGeneration(3),
    };
    let message = WorkerToSupervisor::Hello(Hello::new(worker));
    let json = serde_json::to_string(&message).unwrap();
    assert!(json.contains("\"ffffffffffffffffffffffffffffffff\""));
    assert_eq!(
        serde_json::from_str::<WorkerToSupervisor>(&json).unwrap(),
        message
    );
    assert_eq!(run.to_string().parse::<RunId>().unwrap(), run);
    assert!("+8".parse::<RunId>().is_err());
    assert_eq!("17".parse::<SessionId>().unwrap(), SessionId(17));
}

#[test]
fn frame_round_trips_styled_text_cursor_and_pixels() {
    let mut frame = WindowFrame::new(LayoutGeneration(15), MIN_CONTENT_SIZE);
    frame.cells[0].symbol = "中".into();
    frame.cells[0].foreground = Color::Rgb(80, 160, 242);
    frame.cells[0].background = Color::Indexed(8);
    frame.cells[1].skip = true;
    frame.cells[2].symbol = "e\u{301}".into();
    frame.cursor = Some(Cursor {
        x: 3,
        y: 0,
        shape: CursorShape::SteadyBar,
    });
    frame.images.push(Image {
        image_id: 1,
        width: 1,
        height: 1,
        rgba: [0, 128, 168, 255].into(),
        source: [0, 0, 1, 1],
        padding: [0; 4],
        cell_size: [1, 1],
        area: Rect {
            x: 0,
            y: 1,
            width: 1,
            height: 1,
        },
        visible: vec![Rect {
            x: 0,
            y: 1,
            width: 1,
            height: 1,
        }],
        z_index: 0,
    });
    frame.headings.push(Heading {
        area: Rect {
            x: 4,
            y: 2,
            width: 10,
            height: 2,
        },
        text: "标题".into(),
        level: 1,
        foreground: Color::Indexed(3),
    });
    frame.validate().unwrap();
    let message = AppToShell::Frame(frame);
    let bytes = serde_json::to_vec(&message).unwrap();
    assert_eq!(
        serde_json::from_slice::<AppToShell>(&bytes).unwrap(),
        message
    );
}

#[test]
fn frame_rejects_terminal_control_symbols_and_invalid_geometry() {
    let mut frame = WindowFrame::new(LayoutGeneration(1), Size::new(2, 1));
    frame.cells[0].symbol = "\x1b[1A".into();
    assert_eq!(
        frame.validate().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    frame.cells[0].symbol = "a".into();
    frame.cursor = Some(Cursor {
        x: 2,
        y: 0,
        shape: CursorShape::Default,
    });
    assert_eq!(
        frame.validate().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    frame.cursor = None;
    frame.cells.pop();
    assert_eq!(
        frame.validate().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn repeated_color_cells_compress_before_transport() {
    let mut frame = WindowFrame::new(LayoutGeneration(1), Size::new(120, 34));
    for (position, cell) in frame.cells.iter_mut().enumerate() {
        cell.symbol = char::from(b'a' + (position % 26) as u8).into();
        cell.foreground = Color::Rgb(130, 184, 211);
        cell.background = Color::Rgb(19, 39, 48);
        cell.modifiers = 1;
    }
    let original_size = serde_json::to_vec(&frame.cells).unwrap().len();
    let bytes = serde_json::to_vec(&frame).unwrap();
    assert!(
        bytes.len() < original_size / 20,
        "{} of {original_size} bytes",
        bytes.len()
    );
    assert_eq!(
        serde_json::from_slice::<WindowFrame>(&bytes).unwrap(),
        frame
    );
}

#[test]
fn frame_rejects_corrupted_compressed_cells() {
    let frame = WindowFrame::new(LayoutGeneration(1), Size::new(1, 1));
    let mut json = serde_json::to_value(frame).unwrap();
    let mut compressed = STANDARD.decode(json["cells"].as_str().unwrap()).unwrap();
    let checksum = compressed.len() - 8;
    compressed[checksum] ^= 1;
    json["cells"] = STANDARD.encode(compressed).into();
    assert!(serde_json::from_value::<WindowFrame>(json).is_err());
}

#[test]
fn shared_operations_preserve_native_paths_and_errors() {
    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Paths(#[serde(with = "workspace_protocol::native_paths")] Vec<PathBuf>);

    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![b'/', b'a', 0xff, b'b'])
    };
    #[cfg(windows)]
    let path = {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[b'a' as u16, 0xd800, b'b' as u16])
    };
    let paths = Paths(vec![PathBuf::from(path), PathBuf::from("目录")]);
    let bytes = serde_json::to_vec(&paths).unwrap();
    assert_eq!(serde_json::from_slice::<Paths>(&bytes).unwrap(), paths);
    let reply = ServiceReply {
        id: 22,
        result: Err(ServiceError::OperationFailed {
            message: serde_json::json!({"key": "app-explorer-undo-changed"}),
        }),
    };
    let bytes = serde_json::to_vec(&reply).unwrap();
    assert_eq!(
        serde_json::from_slice::<ServiceReply>(&bytes).unwrap(),
        reply
    );
}

#[test]
fn framing_preserves_split_headers_bodies_and_final_message_before_eof() {
    let wire = [0, 0, 0, 3, b'"', b'z', b'"', 0, 0, 0, 2, b'4', b'2'];
    let input = VecDeque::from([
        Some(wire[..2].to_vec()),
        None,
        Some(wire[2..5].to_vec()),
        None,
        Some(wire[5..].to_vec()),
        Some(Vec::new()),
    ]);
    let mut stream = Framed::new(
        ScriptedIo {
            input,
            output: Vec::new(),
        },
        20,
    );
    assert!(stream.poll::<serde_json::Value>().unwrap().is_empty());
    assert!(stream.poll::<serde_json::Value>().unwrap().is_empty());
    assert_eq!(
        stream.poll::<serde_json::Value>().unwrap(),
        [serde_json::json!("z"), serde_json::json!(42)]
    );
    assert_eq!(
        stream.poll::<serde_json::Value>().unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn framing_bounds_the_write_queue_and_recovers_after_drain() {
    let mut stream = Framed::new(ScriptedIo::default(), 8);
    assert_eq!(
        stream.queue(&"1234567").unwrap_err().kind(),
        io::ErrorKind::InvalidInput
    );
    assert!(!stream.pending_write());
    stream.queue(&"123456").unwrap();
    assert!(stream.pending_write());
    assert_eq!(
        stream.queue(&"next").unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert!(stream.poll::<String>().unwrap().is_empty());
    assert!(!stream.pending_write());
    stream.queue(&"next").unwrap();
    stream.poll::<String>().unwrap();
    assert!(!stream.pending_write());
}

#[test]
fn framing_rejects_oversized_and_truncated_messages() {
    let stream = ScriptedIo {
        input: VecDeque::from([Some(vec![0, 0, 0, 21])]),
        output: Vec::new(),
    };
    assert_eq!(
        Framed::new(stream, 20).poll::<String>().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    let stream = ScriptedIo {
        input: VecDeque::from([Some(vec![0, 0, 0, 3, b'"']), Some(Vec::new())]),
        output: Vec::new(),
    };
    assert_eq!(
        Framed::new(stream, 20).poll::<String>().unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

#[derive(Default)]
struct ScriptedIo {
    input: VecDeque<Option<Vec<u8>>>,
    output: Vec<u8>,
}

impl Read for ScriptedIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(Some(bytes)) = self.input.pop_front() else {
            return Err(io::ErrorKind::WouldBlock.into());
        };
        buf[..bytes.len()].copy_from_slice(&bytes);
        Ok(bytes.len())
    }
}

impl Write for ScriptedIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let size = bytes.len().min(3);
        self.output.extend_from_slice(&bytes[..size]);
        Ok(size)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
