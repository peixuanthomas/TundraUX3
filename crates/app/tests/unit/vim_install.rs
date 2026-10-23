use super::*;
use std::net::TcpListener;

fn temporary_root() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-vim-install-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&root).unwrap();
    root
}

fn bundle() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
    for (name, contents) in [
        (EXECUTABLE, "test executable"),
        ("runtime/defaults.vim", "defaults"),
        ("runtime/doc/help.txt", "help"),
        ("LICENSE", "Vim License"),
        ("UPSTREAM.json", PIN),
    ] {
        zip.start_file(format!("vim/{name}"), options).unwrap();
        zip.write_all(contents.as_bytes()).unwrap();
    }
    #[cfg(windows)]
    for name in super::super::WINDOWS_ICONV_FILES {
        zip.start_file(format!("vim/{name}"), options).unwrap();
        zip.write_all(b"iconv fixture").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn server(archive: Vec<u8>, digest: String) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let checksum = format!("{digest}  engine.zip\n");
    let worker = std::thread::spawn(move || {
        for body in [checksum.into_bytes(), archive] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = [0; 4096];
            stream.read(&mut request).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            stream.write_all(&body).unwrap();
        }
    });
    (url, worker)
}

#[test]
fn downloaded_engine_is_verified_and_replaces_an_incomplete_directory() {
    let root = temporary_root();
    let _owned = WorkDirectory::new(root.clone());
    let directory = root.join("vim");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("incomplete"), "old").unwrap();
    let archive = bundle();
    let (url, worker) = server(archive.clone(), format!("{:x}", Sha256::digest(&archive)));
    let mut progress = Vec::new();
    install_from(
        &format!("{url}/"),
        "engine.zip",
        &directory,
        &mut |event| progress.push(event),
        &Default::default(),
    )
    .unwrap();
    worker.join().unwrap();
    assert!(directory.join(EXECUTABLE).is_file());
    assert!(!directory.join("incomplete").exists());
    assert_eq!(progress.last().unwrap().phase, VimInstallPhase::Verifying);
    assert!(
        progress
            .iter()
            .any(|event| event.phase == VimInstallPhase::Downloading
                && event.downloaded == archive.len() as u64)
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn a_bad_checksum_keeps_the_existing_directory_and_removes_staging() {
    let root = temporary_root();
    let _owned = WorkDirectory::new(root.clone());
    let directory = root.join("vim");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("original"), "keep").unwrap();
    let (url, worker) = server(bundle(), "0".repeat(64));
    let error = install_from(
        &format!("{url}/"),
        "engine.zip",
        &directory,
        &mut |_| {},
        &Default::default(),
    )
    .unwrap_err();
    worker.join().unwrap();
    assert!(error.to_string().contains("SHA-256"));
    assert_eq!(
        fs::read_to_string(directory.join("original")).unwrap(),
        "keep"
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
}

#[test]
fn cancellation_stops_before_network_or_filesystem_changes() {
    let cancel = UpdateCancellation::default();
    cancel.cancel();
    let error = install_from(
        "http://127.0.0.1:1/",
        "engine.zip",
        Path::new("unused"),
        &mut |_| panic!("cancelled install reported progress"),
        &cancel,
    )
    .unwrap_err();
    assert!(error.is_cancelled());
}
