# Ubuntu / Arch 源码包更新验证

2026-09-27，本机 WSL2 Ubuntu 24.04.4 LTS，x86_64，普通用户，Rust 1.98.0。

## 已验证

- `cargo fmt --check`、`git diff --check` 和 `python3 scripts/check-localization.py` 通过。
- Windows `cargo check --workspace --locked` 通过；有两处未修改文件中的已有警告。
- Ubuntu `cargo build --workspace --locked` 通过。
- `cargo test --locked -p app --lib ubuntu_builds_real_deb -- --ignored` 使用真实
  `dpkg-shlibdeps`、`dpkg-deb` 验证包版本、共享库依赖、root 文件归属、程序和资源内容。
- `ubuntu_export_current_build_for_install_smoke` 用当前工作区编译的 Shell/CLI
  导出两个测试 DEB。测试使用 debug 程序；正式更新调用既有的 release 编译流程。
- 使用真实 `apt-get --no-remove --no-install-recommends install` 完成首次安装及第二个
  软件包版本的升级。首次安装仅新增 `tundraux3` 和缺失依赖 `xdg-utils`，升级仅改动
  `tundraux3`。测试驱动显式传入 `-y`，应用实际调用保留包管理器的交互确认。
- 两次安装均通过 `ubuntu_verifies_installed_smoke_package`：安装版本与两个程序的
  提交号匹配；人为传入错误版本或提交号时验证失败。源码程序和系统程序运行
  `tundra-cli debug doctor`，均正确识别为 `SystemDeb`，分别提示首次安装和系统包升级。

## 完整测试中的已有失败

`cargo test --workspace --locked --no-fail-fast` 跑完全部目标，仅失败一项：

```text
shell / account_workflows
linux_attaches_current_user_and_only_requires_first_appearance_setup
left: Language
right: Appearance
```

在修改前的 `89e49f2` 源码快照中单独运行同一测试，出现相同失败。
本次没有修改该测试或首次启动流程，其余工作区测试通过。
为控制 WSL 内存占用，构建使用 `CARGO_BUILD_JOBS=2`、
`CARGO_PROFILE_DEV_DEBUG=0` 和 `CARGO_PROFILE_TEST_DEBUG=0`。

## 尚未验证

- 本机没有 Arch 测试系统，尚未执行真实的 makepkg/pacman 安装。
  已增加 Arch CI 任务和 `arch_builds_real_pacman_package` 测试；CI 尚未运行。
- 尚未在完整界面中手动输入 sudo 密码、取消认证或验证重启后的画面。
  本次真实安装由测试命令发起，安装识别、打包和安装后检查调用应用使用的代码。
- 本次没有执行网络下载、完整 release 编译和界面交互串起来的一次端到端更新。

打包工具依据：[dpkg-deb](https://manpages.debian.org/bookworm/dpkg/dpkg-deb.1.en.html)、
[makepkg](https://man.archlinux.org/man/makepkg.8)、
[pacman](https://man.archlinux.org/man/pacman.8)。
