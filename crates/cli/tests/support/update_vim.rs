use std::fs;
use std::path::Path;

pub fn write_bundle(directory: &Path, program: &[u8]) {
    fs::create_dir_all(directory.join("runtime/doc")).unwrap();
    fs::create_dir_all(directory.join("runtime/syntax")).unwrap();
    let executable = directory.join(if cfg!(windows) { "vim.exe" } else { "vim" });
    fs::write(&executable, program).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(directory.join("runtime/defaults.vim"), b"defaults").unwrap();
    fs::write(directory.join("runtime/doc/help.txt"), b"help").unwrap();
    fs::write(directory.join("runtime/syntax/test.vim"), program).unwrap();
    fs::write(directory.join("LICENSE"), b"Vim License").unwrap();
    fs::write(
        directory.join("UPSTREAM.json"),
        include_str!("../../../../packaging/vim.json"),
    )
    .unwrap();
}
