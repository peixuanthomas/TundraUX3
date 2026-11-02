# Linux 更新测试

当前更新系统面向便携目录。Linux 测试在普通用户环境运行，Windows 开发机使用 WSL：

```sh
cargo test --workspace --locked
```

主要测试位置：

| 文件 | 覆盖内容 |
| --- | --- |
| `crates/app/tests/unit/update_release.rs` | 正式版选择、校验和、损坏内容和便携包提取 |
| `crates/app/tests/unit/update.rs` | 准备、替换、中断恢复和回退状态 |
| `crates/app/tests/unit/update_cancellation.rs` | 取消、清理和竞争状态 |
| `crates/app/tests/unit/vim_install.rs` | Vim 包校验、取消和文件安全 |
| `crates/cli/tests/update_linux.rs` | 真实更新助手、替换、启动失败回退和前台等待 |
| `crates/shell/tests/settings.rs` | 更新模式的键盘、鼠标操作与持久保存 |

固定测试使用内存数据和临时便携目录，不发起 HTTP 请求或启动本地 HTTP 服务器。完整 Release 下载、Beta 构建和真实重启在对应平台的发布验收中检查。

历史系统包验证记录位于 `docs/packaging/linux/VALIDATION-*.md`，当前更新说明以[程序自更新](update.md)为准。
