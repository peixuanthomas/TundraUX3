# vim-runtime

`vim-runtime` 运行真实 Vim，并在 Vim 与 Tundra 编辑器之间同步缓冲、终端画面和保存请求。Vim 的模式、编辑按键、撤销、寄存器、宏、窗口、搜索和 Ex 命令由 Vim 自己处理，本 crate 不重新实现这些功能。

## 进程与通信

`VimHost` 使用 `terminal-runtime` 的 Unix PTY 或 Windows ConPTY 启动 Vim，进程清理由 watchdog 监督。内置 `bridge.vim` 通过带随机令牌认证的本机回环连接交换 JSON 行。

运行需要包含 `+eval`、`+channel`、`+timers` 的完整 Vim。发布包提供 `vim/vim`（Windows 为 `vim/vim.exe`）和 `vim/runtime`，调用方通过 `VimConfig` 指定路径、尺寸、用户配置和初始缓冲。

按键、粘贴、鼠标和焦点使用共享 `PtyProcess` 接口。`set_cell_size` 提供真实字符格的像素尺寸，供图片定位和终端查询使用。

## 缓冲同步

- `open_buffer` 导入文本、编码、换行、BOM、末尾换行、光标和保护标志；相同 `host_id` 更新原缓冲。
- `activate_buffer` 切换当前缓冲并保留 Vim 状态，`remove_buffer` 删除宿主已关闭的文档。
- `request_export` 导出当前内容，请求编号与状态快照用于确认同步完成。
- Vim `listener_add()` 收集行变化，供脚本断点定位；宿主导入不产生用户撤销记录。
- 暂停显示时保留 `VimHost`，从而保留进程、寄存器、宏和撤销历史。

`VimPoll::terminal` 按 PTY 输出版本共享 `Arc` 快照。没有新输出时复用静止画面，输出、尺寸变化和退出后刷新会生成新快照。

## 受控保存

`BufWriteCmd`、`FileWriteCmd` 和 `FileAppendCmd` 将写入请求交给宿主，包含目标路径、缓冲路径、写入范围、追加和强制标志。

宿主完成权限、外部变更、配置差异和异步保存检查后，通过 `respond_save` 回复结果。成功响应才清除修改标记，失败时 `:wq` 留在 Vim。只读缓冲拒绝修改和保存，受保护配置不进入普通 swap、备份或持久撤销文件。

`request_close(false)` 让 Vim 执行未保存检查，`:q!` 和 `:qa!` 按 Vim 规则放弃内容。关闭宿主前先导出并处理保存或放弃，再清理进程。

## 验证

构建完整 Vim 后运行真实运行时回归：

```sh
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored --nocapture
cargo test --locked -p vim-runtime --test terminal_cache
```

测试需要真实 PTY 和本机回环连接，覆盖缓冲导入、跨缓冲撤销、受控保存、`:wq`、强制放弃、只读保护，以及静止画面、输出、缩放和退出刷新。

构建与打包见[随包 Vim](../../docs/packaging/vim.md)，用户操作见[编辑器与脚本工作区](../../docs/source-editor.md#真实-vim-模式)。
