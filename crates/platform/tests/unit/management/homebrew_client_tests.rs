use super::*;

#[test]
fn local_client_keeps_resize_and_explicit_stop_separate_from_terminal_bytes() {
    let (send, inputs) = mpsc::channel();
    send.send(OperationInput::Resize {
        columns: 140,
        rows: 40,
    })
    .unwrap();
    send.send(OperationInput::Terminal {
        bytes: b"y\r".to_vec(),
    })
    .unwrap();
    send.send(OperationInput::Terminate).unwrap();
    let emit = |_| {};
    let mut interaction = Interaction {
        inputs,
        emit: &emit,
        size: None,
        stop: None,
    };
    assert_eq!(
        interaction.terminal_input(Duration::ZERO).unwrap(),
        Some(b"y\r".to_vec())
    );
    assert_eq!(interaction.terminal_size(), Some((140, 40)));
    interaction.terminal_input(Duration::ZERO).unwrap();
    assert_eq!(interaction.stop_requested(), Some(false));
    send.send(OperationInput::Kill).unwrap();
    send.send(OperationInput::Cancel).unwrap();
    interaction.terminal_input(Duration::ZERO).unwrap();
    assert_eq!(interaction.stop_requested(), Some(true));
}
