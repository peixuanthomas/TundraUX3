use super::*;

fn test_path(label: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "tundra-dirs-{label}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn user_dirs_expand_home_and_ignore_relative_values() {
    let root = test_path("user-dirs");
    let _ = std::fs::create_dir_all(&root);
    let path = root.join("user-dirs.dirs");
    std::fs::write(
        &path,
        "XDG_DESKTOP_DIR=\"$HOME/Desk\"\nXDG_DOWNLOAD_DIR=\"relative\"\n",
    )
    .unwrap();
    let dirs = XdgUserDirs::from_file(&path, Path::new("/home/tundra"));
    assert_eq!(dirs.desktop, Some(PathBuf::from("/home/tundra/Desk")));
    assert_eq!(dirs.download, None);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn account_user_dirs_use_each_homes_localized_config_and_fallbacks() {
    let root = test_path("account-user-dirs");
    let first = root.join("first");
    let second = root.join("second");
    fs::create_dir_all(first.join(".config")).unwrap();
    fs::create_dir_all(&second).unwrap();
    fs::write(
        first.join(".config/user-dirs.dirs"),
        "XDG_DESKTOP_DIR=\"$HOME/桌面\"\nXDG_DOCUMENTS_DIR=\"$HOME/文档\"\n\
             XDG_DOWNLOAD_DIR=\"/srv/shared/downloads\"\nXDG_PICTURES_DIR=\"$HOME/图片\"\n\
             XDG_VIDEOS_DIR=\"$HOME/视频\"\nXDG_MUSIC_DIR=\"$HOME/音乐\"\n",
    )
    .unwrap();
    let resolve = |home: &Path| {
        resolve_user_dirs(home, &home.join(".config"), home.join(".local/share")).unwrap()
    };
    let dirs = resolve(&first);
    assert_eq!(dirs.desktop(), first.join("桌面"));
    assert_eq!(dirs.documents(), first.join("文档"));
    assert_eq!(dirs.downloads(), Path::new("/srv/shared/downloads"));
    assert_eq!(dirs.pictures(), first.join("图片"));
    assert_eq!(dirs.videos(), first.join("视频"));
    assert_eq!(dirs.music(), first.join("音乐"));
    assert_eq!(dirs.app_data(), first.join(".local/share"));
    let dirs = resolve(&second);
    assert_eq!(dirs.desktop(), second.join("Desktop"));
    assert_eq!(dirs.documents(), second.join("Documents"));
    assert_eq!(dirs.app_data(), second.join(".local/share"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn user_dirs_reject_unquoted_values_and_partial_home_expansion() {
    let root = test_path("user-dirs-invalid");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("user-dirs.dirs");
    fs::write(
        &path,
        "XDG_DESKTOP_DIR=$HOME/Desktop\nXDG_DOWNLOAD_DIR=\"$HOMEevil/Downloads\"\n",
    )
    .unwrap();
    let dirs = XdgUserDirs::from_file(&path, Path::new("/home/tundra"));
    assert_eq!(dirs.desktop, None);
    assert_eq!(dirs.download, None);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn missing_config_resolves_existing_chinese_personal_folders() {
    let root = test_path("missing-config");
    for folder in ["桌面", "文档", "下载", "图片", "视频", "音乐"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    let dirs = resolve_user_dirs(&root, &root.join(".config"), root.join(".local/share")).unwrap();
    let actual = [
        dirs.desktop(),
        dirs.documents(),
        dirs.downloads(),
        dirs.pictures(),
        dirs.videos(),
        dirs.music(),
    ];
    for (path, folder) in actual
        .into_iter()
        .zip(["桌面", "文档", "下载", "图片", "视频", "音乐"])
    {
        assert_eq!(path, root.join(folder));
        assert!(path.is_dir());
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn configured_paths_accept_comments_and_braced_home() {
    let root = test_path("config-syntax");
    fs::create_dir_all(root.join(".config")).unwrap();
    fs::write(
        root.join(".config/user-dirs.dirs"),
        "XDG_DESKTOP_DIR=\"$HOME/桌面\" # desktop\nXDG_DOCUMENTS_DIR=\"${HOME}/文档\"\n",
    )
    .unwrap();
    let dirs = resolve_user_dirs(&root, &root.join(".config"), root.join(".local/share")).unwrap();
    assert_eq!(dirs.desktop(), root.join("桌面"));
    assert_eq!(dirs.documents(), root.join("文档"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fallback_ignores_files_and_finds_traditional_chinese_directories() {
    let root = test_path("traditional");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("Documents"), "not a directory").unwrap();
    for folder in ["桌面", "文件", "下載", "圖片", "影片", "音樂"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    let dirs = resolve_user_dirs(&root, &root.join(".config"), root.join(".local/share")).unwrap();
    for (path, folder) in [
        dirs.desktop(),
        dirs.documents(),
        dirs.downloads(),
        dirs.pictures(),
        dirs.videos(),
        dirs.music(),
    ]
    .into_iter()
    .zip(["桌面", "文件", "下載", "圖片", "影片", "音樂"])
    {
        assert_eq!(path, root.join(folder));
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_custom_and_disabled_paths_win_over_existing_fallbacks() {
    let root = test_path("configured-precedence");
    fs::create_dir_all(root.join(".config")).unwrap();
    for folder in ["Desktop", "桌面", "Documents", "文档", "Downloads", "下载"] {
        fs::create_dir_all(root.join(folder)).unwrap();
    }
    fs::write(
        root.join(".config/user-dirs.dirs"),
        "XDG_DESKTOP_DIR=\"$HOME/\"\nXDG_DOCUMENTS_DIR=\"$HOME/custom-offline\"\n",
    )
    .unwrap();
    let dirs = resolve_user_dirs(&root, &root.join(".config"), root.join(".local/share")).unwrap();
    assert_eq!(dirs.desktop(), root);
    assert_eq!(dirs.documents(), root.join("custom-offline"));
    assert_eq!(dirs.downloads(), root.join("Downloads"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn parser_preserves_escaped_path_characters_without_evaluating_shell_code() {
    let home = Path::new("/home/peixuan");
    for (value, expected) in [
        (r#""$HOME""#, "/home/peixuan"),
        (r#""${HOME}/""#, "/home/peixuan/"),
        (r#""$HOME/Desk # 1" # comment"#, "/home/peixuan/Desk # 1"),
        (r#""/srv/\$HOME/\`literal\`""#, "/srv/$HOME/`literal`"),
        (r#""$HOME/with\q""#, "/home/peixuan/with\\q"),
    ] {
        assert_eq!(
            parse_user_dir_value(value, home),
            Some(PathBuf::from(expected)),
            "{value}"
        );
    }
    assert_eq!(
        parse_user_dir_value(r#""/srv/My \"Desk\"""#, home),
        Some(PathBuf::from("/srv/My \"Desk\""))
    );
    for value in [
        r#""$HOMEevil/Desktop""#,
        r#""${HOME}evil/Desktop""#,
        r#""\$HOME/Desktop""#,
        r#""relative""#,
        r#""$HOME/Desktop" garbage"#,
        r#""$HOME/Desktop"; touch /tmp/unwanted"#,
        r#""$HOME/$(id)""#,
        r#""/srv/`id`""#,
        r#""$HOME/unclosed"#,
    ] {
        assert_eq!(parse_user_dir_value(value, home), None, "{value}");
    }
}
