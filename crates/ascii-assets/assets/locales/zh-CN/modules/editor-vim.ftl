ui-editor-vim-toggle = VIM_mode
ui-editor-vim-return = F8 返回编辑器
ui-editor-vim-off = { " F8 VIM_mode: 关闭 " }
ui-editor-vim-normal = { " F8 VIM_mode: 普通 " }
ui-editor-vim-insert = { " F8 VIM_mode: 输入 " }
ui-editor-vim-help = Vim 帮助
ui-editor-vim-help-text =
    Vim 模式运行随程序提供的 Vim。使用 :help 或 F1 查看完整帮助。
    普通、输入、替换、Visual 和命令行模式，以及移动、操作符、文本对象、寄存器、宏与撤销，均由 Vim 处理。
    :write 保存；:wq 保存并退出；:quit 退出；:quit! 丢弃修改。Ctrl+W 操作 Vim 窗口。
    F8 返回普通编辑器，再按 F8 恢复 Vim，保留寄存器和撤销历史。开关设置会保存供下次使用。
shell-vim-runtime-error = Vim 无法继续：{ $reason }。已同步的文档仍保留在编辑器中。
shell-vim-startup-failed = Vim 未连接到编辑器。需要包含 channel、eval、autocmd 和 timer 支持的完整 Vim。
shell-vim-exited-unexpectedly = Vim 意外停止（退出码 { $code }）。已同步的未保存内容仍保留。
shell-vim-buffer-unavailable = 此 Vim 缓冲区对应的文档已不可用。
shell-vim-file-name-required = E32：没有文件名。请使用 :write 指定路径。
shell-vim-save-failed = 编辑器无法启动此次写入。Vim 缓冲区仍保留未保存状态。
shell-vim-save-cancelled = 已取消配置写入。Vim 缓冲区仍保留未保存状态。
shell-vim-engine-missing = 未找到随程序提供的 Vim 引擎：{ $path }。请使用 scripts/build-vim.py 构建，或使用完整便携包。
shell-vim-config-whole-write = 配置文件需要整份缓冲区写入，并经过差异预览、检查和授权。
shell-vim-unsupported-editor-encoding = 普通编辑器无法保留 { $encoding } 编码。请继续使用 Vim，或先执行 :set fileencoding=utf-8 再按 F8。
shell-vim-save-suspended = 普通编辑器接管文档时，Vim 暂停写入。请按 F8 恢复 Vim 后再写入。
shell-vim-config-text-write = 配置检查需要当前 UTF-8 文本，不能带 BOM 或在写入时转换字节格式。如需其他编码的副本，请写入普通文件。
shell-vim-unsupported-editor-bom = 普通编辑器要求 UTF-16 带 BOM。请继续使用 Vim，或先执行 :set bomb 再按 F8。
