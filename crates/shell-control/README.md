# shell-control

定义外部 CLI 与运行中 Shell 共用的 JSON 请求、响应、界面树和快照。此 crate 不读取配置、不连接终端、不执行系统操作。坐标以终端字符格为单位，左上角是 `(0, 0)`。

连接由 platform 提供；Shell 在实际绘制后发布快照，并把请求转换为正常键盘、鼠标和粘贴输入。协议版本为 1，错误代码不随语言改变。

验证：`cargo test --locked -p shell-control`。
