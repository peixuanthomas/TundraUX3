use super::*;

#[test]
fn queued_packets_reach_the_physical_writer_in_order_and_close_wakes_it() {
    struct Records(Arc<Mutex<Vec<u8>>>, std::sync::mpsc::Sender<()>);
    impl Write for Records {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            self.1.send(()).unwrap();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let queue = Arc::new(InputQueue::default());
    let mut writer = queue.writer();
    writer.write_all(b"key").unwrap();
    writer.write_all(b"reply").unwrap();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (sent, received) = std::sync::mpsc::channel();
    let worker_queue = queue.clone();
    let output = captured.clone();
    let thread = std::thread::spawn(move || worker_queue.run(Box::new(Records(output, sent))));
    received
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    received
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    queue.close();
    thread.join().unwrap();
    assert_eq!(&*captured.lock().unwrap(), b"keyreply");
    assert_eq!(
        writer.write_all(b"late").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[test]
fn queue_overflow_is_visible_even_when_a_terminal_protocol_ignores_write_errors() {
    let queue = Arc::new(InputQueue::default());
    let mut writer = queue.writer();
    writer.write_all(&vec![0; MAX_INPUT_BYTES]).unwrap();
    assert_eq!(
        writer.write_all(b"reply").unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    assert_eq!(
        queue.check_error().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
    queue.close();
}

#[test]
fn blocked_physical_input_does_not_block_parsing_or_a_terminal_snapshot() {
    struct Blocks(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
    impl Write for Blocks {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.send(()).unwrap();
            let _ = self.1.recv();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let queue = Arc::new(InputQueue::default());
    let (started, started_rx) = std::sync::mpsc::channel();
    let (resume, resume_rx) = std::sync::mpsc::channel();
    let worker_queue = queue.clone();
    let worker = std::thread::spawn(move || worker_queue.run(Box::new(Blocks(started, resume_rx))));
    let writer = Arc::new(Mutex::new(queue.writer()));
    writer.lock().unwrap().write_all(b"input").unwrap();
    started_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .unwrap();
    let (completed, result) = std::sync::mpsc::channel();
    let model = std::thread::spawn(move || {
        let mut terminal = crate::emulator::Emulator::new(2, 20, 0, writer.clone());
        terminal.process(b"output\x1b[6n");
        completed.send(terminal.snapshot()).unwrap();
    });
    let snapshot = result.recv_timeout(std::time::Duration::from_secs(2));
    queue.close();
    resume.send(()).unwrap();
    worker.join().unwrap();
    model.join().unwrap();
    assert!(
        snapshot.unwrap().cells[0]
            .iter()
            .map(|cell| cell.text.as_str())
            .collect::<String>()
            .starts_with("output")
    );
}

#[test]
fn physical_writer_errors_are_visible_to_the_owner() {
    struct Fails;
    impl Write for Fails {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let queue = Arc::new(InputQueue::default());
    queue.writer().write_all(b"key").unwrap();
    queue.run(Box::new(Fails));
    assert_eq!(
        queue.check_error().unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        queue.writer().write_all(b"late").unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
}
