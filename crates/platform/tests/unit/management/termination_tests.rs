use super::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn child(script: &str) -> ChildGuard {
    let mut child = Command::new("/bin/sh")
        .args(["-c", script])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut ready = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert_eq!(ready.trim(), "ready");
    ChildGuard(child)
}

fn await_exit(tree: &ProcessTree) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while tree.has_live_children() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!tree.has_live_children());
}

#[test]
fn termination_signals_the_owned_process_and_does_not_touch_a_sibling() {
    let mut target = child("echo ready; exec sleep 60");
    let mut sibling = child("echo ready; exec sleep 60");
    let mut tree = ProcessTree::child(target.0.id()).unwrap();
    tree.signal(false).unwrap();
    await_exit(&tree);
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(target.0.wait().unwrap().signal(), Some(libc::SIGTERM));
    assert!(sibling.0.try_wait().unwrap().is_none());
    // A completed/reaped process is harmless, including a delayed kill click.
    tree.signal(true).unwrap();
    assert!(sibling.0.try_wait().unwrap().is_none());
}

#[test]
fn ignored_term_stays_alive_until_an_explicit_kill_including_descendants() {
    let mut target = child("trap '' TERM; sleep 60 & echo ready; wait");
    let mut tree = ProcessTree::child(target.0.id()).unwrap();
    tree.signal(false).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    assert!(tree.has_live_children());
    assert!(target.0.try_wait().unwrap().is_none());
    assert!(
        tree.processes.len() >= 2,
        "the background child must be tracked"
    );
    tree.signal(true).unwrap();
    await_exit(&tree);
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(target.0.wait().unwrap().signal(), Some(libc::SIGKILL));
}
