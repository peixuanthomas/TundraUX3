# script-runtime

Linux 脚本运行与 bashdb 调试后端。普通运行按调用方提供的解释器、参数和目录启动；调试要求 Bash 5.2 以上。两种方式都复用 `terminal-runtime` 的 PTY、输出解析和 watchdog 进程树清理。

运行进程使用共享 `PtyConfig` 与 `PtyProcess`；交互输出的按键和粘贴通过其独立输入方法发送，遵守子程序当前选择的终端模式。

`ScriptSession::spawn` 创建一轮运行；`command` 接收继续、步进、断点和表达式命令，`poll` 返回启动、暂停快照、表达式结果和退出码。查询通过私有通道异步完成，不在 UI 线程等待调试器回应。Shell 负责文档保存、页实例检查、只读状态、Vim 切换和界面。

结束后 `into_output` 清理进程、PTY 和调试临时目录，只保留支持回滚与调整尺寸的终端输出；`into_final_snapshot` 继续提供最后一屏快照。

运行时可注册输出接收函数，把新收到的原始输出逐块交给 Shell 的任务历史；终端画面和输入处理不变，停止后仍等进程清理完才报告完成。

固定 bashdb 5.2-1.2.0 的脚本和许可证编译进程序，调试时写入私有临时目录，停止及销毁时清理。用户脚本通过原路径运行，BASH_ENV 只加载调试器，不以 source 重新执行用户脚本，保留 `$0`。调试命令使用独立 FIFO，输出使用独立文件，不占用用户脚本的 stdin/stdout/stderr。

源码与本地修正在 `third_party/bashdb`；普通 Cargo 构建不下载文件。变量和数组保留 Bash 声明中的转义，不通过执行变量值解析数据。

bashdb 只检查断点行号范围，没有完整的可执行行清单。本后端因此只在实际暂停到相应文件和行时将断点标为已验证，其余已接受断点保持“待验证”；调试器明确拒绝的空行、注释、结束关键字和函数声明显示“未生效”。重新设置断点后清除旧的拒绝结果，等待本轮确认。同一轮调试保留已观察到的行，重新启动时重置，不通过提前执行脚本探测可执行行。

Linux/WSL 验证：`cargo test --locked -p script-runtime`。真实测试覆盖 dash 参数与输入、退出码，Bash 初始暂停、函数断点、变量、数组、source 文件步进、`$?`、并行会话隔离、子进程清理和表达式输出隔离。`python3 crates/script-runtime/tests/native_bridge.py` 另验 Bash `-e -u`、`set -euo pipefail` 和 bashdb 原生命令的空格及转义保真。
