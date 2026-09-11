//! A non-replayable reader for one protected conversation.
use crate::channel::{self, ServerMessage};
use std::{
    io::{self, BufReader, Read},
    net::Shutdown,
    os::unix::net::UnixStream,
    sync::mpsc::{self, Receiver, TryRecvError},
};
use watchdog::{ManagedTaskGroup, ManagedThreadHandle, TaskId, TaskSpec};

pub struct ChannelReader {
    socket: UnixStream,
    messages: Option<Receiver<io::Result<ServerMessage>>>,
    worker: Option<ManagedThreadHandle<()>>,
}

impl ChannelReader {
    pub fn new(socket: UnixStream, group: &ManagedTaskGroup) -> io::Result<Self> {
        Self::start(socket.try_clone()?, socket, group)
    }

    fn start(
        reader: impl Read + Send + 'static,
        socket: UnixStream,
        group: &ManagedTaskGroup,
    ) -> io::Result<Self> {
        let (sender, messages) = mpsc::sync_channel(8);
        // Keep transport copies bounded and avoid retaining previous frames.
        let reader = BufReader::with_capacity(1, reader);
        let mut source = Some((reader, sender));
        let worker = group
            .spawn_thread(
                TaskSpec::one_shot(TaskId::from_static("channel-reader")),
                move || {
                    // Own the sender in this invocation: unwind disconnects the UI
                    // immediately, before watchdog incident reporting can block.
                    let (mut reader, sender) =
                        source.take().expect("channel reader cannot be replayed");
                    loop {
                        let message = channel::read_frame::<ServerMessage>(&mut reader);
                        let done =
                            message.is_err() || matches!(&message, Ok(ServerMessage::Complete {}));
                        if sender.send(message).is_err() || done {
                            break;
                        }
                    }
                },
            )
            .map_err(io::Error::other)?;
        Ok(Self {
            socket,
            messages: Some(messages),
            worker: Some(worker),
        })
    }

    pub fn try_message(&self) -> io::Result<Option<ServerMessage>> {
        match self.messages.as_ref().expect("reader is live").try_recv() {
            Ok(message) => message.map(Some),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "protected conversation reader stopped",
            )),
        }
    }
}

impl Drop for ChannelReader {
    fn drop(&mut self) {
        // Unblock both a pending read and a full queue before joining. This also
        // closes the sessiond conversation on terminal/error/panic unwinding.
        let _ = self.socket.shutdown(Shutdown::Both);
        drop(self.messages.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_message(reader: &ChannelReader) -> io::Result<ServerMessage> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Some(message) = reader.try_message()? {
                return Ok(message);
            }
            assert!(Instant::now() < deadline, "reader failed to finish");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn reader_fails_closed_on_disconnect_and_panic_and_never_replays() {
        use watchdog::{AppCriticality, AppDescriptor, AppId, WatchdogConfig, WatchdogRuntime};
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let config = WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("state"),
            "greeter-reader-test",
            "test",
        )
        .with_unclean_exit_tracking(false);
        let (_runtime, process) = WatchdogRuntime::start(config).unwrap();
        let app = process
            .register_app(AppDescriptor::new(
                AppId::from_static("greeter"),
                "Greeter",
                "test",
                AppCriticality::SessionCritical,
            ))
            .unwrap();
        let group = app.task_group("protected-channel");

        let (socket, peer) = UnixStream::pair().unwrap();
        let reader = ChannelReader::new(socket, &group).unwrap();
        drop(peer);
        assert_eq!(
            wait_message(&reader).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        drop(reader);

        struct PanicReader(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Read for PanicReader {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                panic!("injected reader failure");
            }
        }
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (socket, _peer) = UnixStream::pair().unwrap();
        let reader = ChannelReader::start(PanicReader(attempts.clone()), socket, &group).unwrap();
        assert_eq!(
            wait_message(&reader).unwrap_err().kind(),
            io::ErrorKind::BrokenPipe
        );
        drop(reader);
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);

        let (socket, mut peer) = UnixStream::pair().unwrap();
        let reader = ChannelReader::new(socket, &group).unwrap();
        channel::write_frame(&mut peer, &ServerMessage::Complete {}).unwrap();
        assert!(matches!(
            wait_message(&reader).unwrap(),
            ServerMessage::Complete {}
        ));
        drop(reader);
        let mut byte = [0];
        assert_eq!(peer.read(&mut byte).unwrap(), 0);
    }
}
