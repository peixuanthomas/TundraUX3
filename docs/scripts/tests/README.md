# Linux 更新测试

当前更新系统只支持便携目录。PackageKit、DEB/Arch 更新代码及其专用夹具已删除。

在普通用户的 Linux 环境运行（Windows 开发机使用 WSL）：

```sh
cargo test --workspace --locked
```

`crates/app/tests/unit/update_release.rs` 覆盖正式版选择、校验和、损坏下载和便携包提取。
`crates/app/tests/unit/update.rs` 覆盖准备、替换、中断恢复和回退状态。
`crates/cli/tests/update_linux.rs` 启动真实更新助手，验证替换成功、首次启动失败后的回退及前台等待。
`crates/shell/tests/settings.rs` 验证设置的键盘与鼠标操作、模式切换和持久保存。

历史系统包验证记录位于 `docs/packaging/linux/VALIDATION-*.md`，不代表当前支持系统包更新。
