use super::wait_for_replacement;
use std::io::{BufRead, Read, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const DEPTH_ENV: &str = "TUNDRA_RESTART_TEST_DEPTH";
const EXIT_ENV: &str = "TUNDRA_RESTART_TEST_EXIT";

fn fixture_command(depth: u32, exit_code: i32) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "restart_tests::replacement_fixture",
            "--nocapture",
        ])
        .env(DEPTH_ENV, depth.to_string())
        .env(EXIT_ENV, exit_code.to_string());
    command
}

#[test]
fn replacement_fixture() {
    let Ok(depth) = std::env::var(DEPTH_ENV) else {
        return;
    };
    let depth: u32 = depth.parse().unwrap();
    let exit_code: i32 = std::env::var(EXIT_ENV).unwrap().parse().unwrap();
    if depth > 0 {
        std::process::exit(wait_for_replacement(fixture_command(depth - 1, exit_code)).unwrap());
    }
    println!("REPLACEMENT_READY");
    std::io::stdout().flush().unwrap();
    // The outer test holds stdin open until it has checked that every
    // restarting ancestor is still alive. EOF then lets the new UI exit.
    std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
    std::process::exit(exit_code);
}

#[test]
fn windows_restart_keeps_caller_waiting_and_propagates_exit_code() {
    for (depth, exit_code) in [(1, 0), (3, 23)] {
        let mut child = fixture_command(depth, exit_code)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = std::io::BufReader::new(child.stdout.take().unwrap());
        assert!(
            output
                .lines()
                .any(|line| line.unwrap() == "REPLACEMENT_READY")
        );
        let (sender, receiver) = mpsc::channel();
        let waiter = std::thread::spawn(move || sender.send(child.wait()).unwrap());
        let premature_exit = receiver.recv_timeout(Duration::from_millis(250));
        let stayed_alive = matches!(premature_exit, Err(mpsc::RecvTimeoutError::Timeout));
        drop(input);
        let status = match premature_exit {
            Ok(status) => status,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                receiver.recv_timeout(Duration::from_secs(10)).unwrap()
            }
            Err(error) => panic!("replacement waiter disconnected: {error}"),
        }
        .unwrap();
        waiter.join().unwrap();
        assert!(stayed_alive, "restart released the invoking shell early");
        assert_eq!(status.code(), Some(exit_code));
    }
}
