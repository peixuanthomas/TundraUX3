use super::*;
#[test]
fn update_toolchain_finds_current_users_default_tools_outside_path() {
    let root = std::env::temp_dir().join(format!("tundra-toolchain-{}", std::process::id()));
    let home = root.join("invoking-user/.rustup");
    let bin = home.join("toolchains/stable-test/bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        home.join("settings.toml"),
        "default_toolchain = \"stable-test\"\n",
    )
    .unwrap();
    for name in ["cargo", "rustc"] {
        std::fs::write(
            bin.join(format!("{name}{}", std::env::consts::EXE_SUFFIX)),
            "tool",
        )
        .unwrap();
    }
    let toolchain = Toolchain::from_locations(std::iter::empty(), &[home]).unwrap();
    assert_eq!(
        toolchain.spec("cargo").program().parent(),
        Some(bin.as_path())
    );
    assert_eq!(
        Path::new(toolchain.spec("cargo").env_map().get("RUSTC").unwrap()),
        bin.join(format!("rustc{}", std::env::consts::EXE_SUFFIX))
    );
    assert!(Toolchain::from_locations(std::iter::empty(), &[]).is_err());
    std::fs::remove_dir_all(root).unwrap();
}
