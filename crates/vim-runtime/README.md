# vim-runtime

启动时将内置 `bridge.vim` 的换行统一为 LF，避免 Windows 检出的 CRLF 在隔离运行的 Unix Vim 中被当成命令字符；此处理只作用于启动脚本，不改变用户文档。

运行真正的 Vim，Vim 开启期间由 Vim 管理文本、模式、撤销、寄存器、宏、窗口、搜索和 Ex 命令；本 crate 不解释 Vim 编辑按键。Shell 只负责输入路由、页面生命周期和保存授权，UI 绘制真实 PTY 快照。

`VimHost` 使用已有 `terminal-runtime` 的 Unix PTY / Windows ConPTY 与 watchdog 进程树清理。初始化脚本通过带随机令牌认证的本机回环连接交换 JSON 行；缓冲内容不写入 IPC 文件、日志或临时脚本。运行时需要包含 `+eval`、`+channel`、`+timers` 的完整 Vim；发布包在程序同目录提供 `vim/vim`（Windows 为 `vim/vim.exe`）及 `vim/runtime`，由调用方指定路径。

`open_buffer` 导入未保存文本与编码、换行、BOM、末尾换行、光标和保护标志；同 `host_id` 更新原缓冲，相同文本不重建撤销历史。`activate_buffer` 只切换，保留 Vim 状态。状态快照携带全部已加载的普通缓冲，正文按变化发送并共享内存；`request_export` 的编号可与 `state.last_command` 对应，用于确认暂停前同步完成。

`BufWriteCmd`、`FileWriteCmd`、`FileAppendCmd` 将保存请求交给宿主，包含实际目标、当前缓冲路径、范围/追加与强制标志。宿主调用现有文件权限、差异检查和异步保存流程后，通过 `respond_save` 回应；只有成功回应才清除相应修改标记，失败会使 `:wq` 留在 Vim。脚本不直接写目标文件。只读缓冲禁止修改和保存，自动 swap、备份和持久撤销均关闭，保护配置不会进入普通恢复文件。

`request_close(false)` 由 Vim 自己检查未保存缓冲并显示 E37；正常 `:q!` / `:qa!` 退出状态明确告知宿主丢弃未保存内容。暂停时不要销毁 `VimHost`，以保留寄存器、宏和撤销；关闭宿主前先导出并处理保存/放弃，再清理进程。

真实运行时回归：

```console
TUNDRA_TEST_VIM="$PWD/target/debug/vim/vim" cargo test --locked -p vim-runtime --test real_vim -- --ignored --nocapture
```

测试需要真实 PTY 和本机回环网络；受限开发沙箱必须允许回环连接。覆盖初始缓冲不进入撤销、真实 Vim 编辑/跨缓冲撤销、受控保存失败和成功、`:wq` 等待、强制丢弃，以及只读保护。不能用 tiny Vim 或仅编译通过代替完整运行时验证。
