use super::*;

#[test]
fn release_check_resolves_the_tag_and_uses_release_notes() {
    use std::io::{BufRead, BufReader};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let api_root = format!("http://{}", listener.local_addr().unwrap());
    let sha = "c".repeat(40);
    let commit = sha.clone();
    let server = std::thread::spawn(move || {
        let release = release();
        let assets: Vec<_> = release
            .assets
            .iter()
            .map(|a| {
                serde_json::json!({
                    "name": a.name, "browser_download_url": a.browser_download_url, "state": a.state
                })
            })
            .collect();
        for (path, body) in [
            (
                "releases/latest",
                serde_json::json!({"tag_name":"v1.3.2", "draft":false,
                "prerelease":false, "body":"Release notes", "target_commitish":"master", "assets":assets}),
            ),
            ("commits/v1.3.2", serde_json::json!({"sha":commit})),
        ] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(&mut stream).read_line(&mut line).unwrap();
            assert!(line.starts_with(&format!("GET /repos/{GITHUB_OWNER}/{GITHUB_REPO}/{path} ")));
            let body = body.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        }
    });
    let identity = BuildIdentity {
        package_version: "1.3.1".into(),
        commit_sha: Some("a".repeat(40)),
        dirty: false,
    };
    let result = check(&identity, &api_root).unwrap();
    server.join().unwrap();
    assert_eq!(result.head_sha, sha);
    assert_eq!(result.default_branch, "v1.3.2");
    assert_eq!(result.commits[0].message, "Release notes");
    assert_eq!(result.relation, UpdateRelation::Behind { remote_ahead: 1 });
    assert!(result.release.is_some());
}

fn release() -> Release {
    let base = format!("https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}/releases/download/v1.3.2/");
    Release {
        tag_name: "v1.3.2".into(),
        draft: false,
        prerelease: false,
        body: None,
        assets: [
            "TundraUX3-v1.3.2-linux-x86_64.tar.gz",
            "TundraUX3-v1.3.2-linux-x86_64.sha256",
        ]
        .into_iter()
        .map(|name| Asset {
            name: name.into(),
            browser_download_url: format!("{base}{name}"),
            state: "uploaded".into(),
        })
        .collect(),
    }
}

#[test]
fn release_selects_published_linux_archive_and_checksum() {
    let mut value = release();
    assert_eq!(select_payload(&value).unwrap().version, "1.3.2");
    value.prerelease = true;
    assert!(select_payload(&value).is_err());
    value.prerelease = false;
    value.assets.pop();
    assert!(select_payload(&value).is_err());
    let mut value = release();
    value.assets[0].browser_download_url = "https://example.com/payload".into();
    assert!(select_payload(&value).is_err());
}

#[test]
fn release_version_and_commit_both_matter() {
    let identity = BuildIdentity {
        package_version: "1.3.2".into(),
        commit_sha: Some("a".repeat(40)),
        dirty: false,
    };
    let local = Version::new(1, 3, 2);
    assert_eq!(
        release_relation(&identity, &local, &local, &"a".repeat(40)),
        UpdateRelation::Identical
    );
    assert_eq!(
        release_relation(&identity, &local, &Version::new(1, 3, 3), &"b".repeat(40)),
        UpdateRelation::Behind { remote_ahead: 1 }
    );
    assert_eq!(
        release_relation(&identity, &local, &Version::new(1, 3, 1), &"b".repeat(40)),
        UpdateRelation::Ahead { local_ahead: 1 }
    );
    assert_eq!(
        release_relation(&identity, &local, &local, &"b".repeat(40)),
        UpdateRelation::Unknown
    );
}

#[test]
fn checksums_reject_wrong_file_duplicate_and_corrupt_download() {
    let hash = format!("{:x}", Sha256::digest(b"archive"));
    assert_eq!(
        checksum_for(&format!("{hash}  file.tar.gz\n"), "file.tar.gz").unwrap(),
        hash
    );
    assert!(checksum_for(&format!("{hash}  other.tar.gz\n"), "file.tar.gz").is_err());
    assert!(
        checksum_for(
            &format!("{hash}  file.tar.gz\n{hash} *file.tar.gz\n"),
            "file.tar.gz"
        )
        .is_err()
    );
    let mut output = Vec::new();
    download_archive(&b"archive"[..], &mut output, Some(7), &hash, &mut |_| {}).unwrap();
    assert_eq!(output, b"archive");
    assert!(download_archive(&b"corrupt"[..], Vec::new(), None, &hash, &mut |_| {}).is_err());
}

fn archive(files: &[(&str, tar::EntryType)]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(encoder);
    for (name, kind) in files {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(*kind);
        header.set_mode(0o755);
        header.set_size(7);
        header.set_cksum();
        tar.append_data(&mut header, name, &b"fixture"[..]).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

#[test]
fn extracts_only_binaries_and_rejects_missing_duplicate_or_linked_binary() {
    let regular = tar::EntryType::Regular;
    for (files, success) in [
        (
            vec![
                ("portable/tundra-shell", regular),
                ("portable/tundra-cli", regular),
                ("portable/assets/user.txt", regular),
            ],
            true,
        ),
        (vec![("portable/tundra-shell", regular)], false),
        (
            vec![
                ("portable/tundra-shell", regular),
                ("portable/tundra-shell", regular),
            ],
            false,
        ),
        (
            vec![("portable/tundra-shell", tar::EntryType::Symlink)],
            false,
        ),
        (
            vec![
                ("portable/tundra-shell", regular),
                ("other/tundra-cli", regular),
            ],
            false,
        ),
    ] {
        let root = super::super::tests::update_test_root("release-archive");
        fs::create_dir_all(&root).unwrap();
        assert_eq!(
            extract_executables(&archive(&files)[..], &root).is_ok(),
            success
        );
        assert!(!root.join("assets").exists());
        if success {
            assert_eq!(fs::read(root.join(SHELL_FILE)).unwrap(), b"fixture");
            assert_ne!(
                fs::metadata(root.join(SHELL_FILE))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
