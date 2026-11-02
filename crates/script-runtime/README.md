# script-runtime

`script-runtime` 提供 Linux 脚本运行与 bashdb 调试后端。普通运行使用调用方指定的解释器、参数和目录，断点调试需要 Bash 5.2 或更新版本。

## 会话接口

`ScriptSession::spawn` 创建一轮运行，`command` 接收 `RuntimeCommand`，`poll` 返回 `RuntimeEvent`：

- 启动、运行、暂停、表达式结果、退出和失败；
- 继续、单步、步入、跳出、断点和表达式命令；
- 暂停位置、变量、调用栈和断点状态。

查询通过私有通道异步完成，不在 UI 线程等待调试器。Shell 负责文档保存、页面实例、只读状态、Vim 切换和界面。

`terminal()` 提供共享 `PtyProcess`，按键和粘贴按子程序当前终端模式发送。输出接收函数可将原始输出逐块交给任务历史。

`into_output` 清理进程、PTY 和调试临时目录，返回支持回滚与缩放的终端输出；`into_final_snapshot` 返回最后一屏。

## bashdb 集成

固定 bashdb 5.2-1.2.0 的脚本与许可证编译进程序，调试时写入私有临时目录。源码和本地修正在 `third_party/bashdb`，普通 Cargo 构建不下载调试器。

用户脚本通过原路径运行，BASH_ENV 只加载调试器，保留 `$0`。调试命令使用独立 FIFO，输出使用独立文件，不占用用户脚本的 stdin/stdout/stderr。变量与数组按 Bash 声明解析，不执行变量值。

bashdb 不提供完整可执行行清单，因此断点在实际暂停到对应位置后才标记为已验证；调试器拒绝的位置显示“未生效”，其余保持“待验证”。新一轮调试重置这些状态。

## 验证

```sh
cargo test --locked -p script-runtime
python3 crates/script-runtime/tests/native_bridge.py
```

真实测试覆盖解释器参数与输入、退出码、函数断点、变量、数组、`source` 文件、表达式、并行会话和子进程清理。界面操作见[脚本工作区](../../docs/source-editor.md#linux-脚本工作区)。
