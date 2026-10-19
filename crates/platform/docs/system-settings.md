# 操作系统设置

从“设置”的“操作系统设置 [F8]”分组进入。原有选项放在“Tundra 设置”分组，“系统”分类改名为“运行与提醒”。主机名和系统时区通过 systemd 的标准 DBus 接口修改；时区列表来自本机，支持查找。系统时间显示刷新时的实际值，自动校时、实际同步状态和当前服务器分别展示。

本页实际依赖以 systemd 为 PID 1 的 Linux 环境、系统 D-Bus，以及 `org.freedesktop.hostname1`、`org.freedesktop.timedate1` 和 systemd 管理接口。主机名直接使用 D-Bus，不依赖 `hostnamectl`；`timedatectl list-timezones` 只提供时区选项。timesyncd 配置查询还需要 `systemd-analyze cat-config`；chrony 读取其实际配置，`chronyc` 只用于查询当前来源，未取得时显示“尚未获取”。本机时间读取失败时沿用同次快照的 UTC 时间。

缺少 systemd 或系统总线时，不将环境限制显示为正常空数据，也不建议授权。查询失败沿管理页的 F4“查看详情”保留具体原因、受限操作和原始诊断。缺工具、明确拒绝访问和其他执行失败分别报告；时区列表或校时配置单项失败时保留已读的主机名、时区与同步状态，仅限制依赖失败项的操作。

校时支持 systemd-timesyncd 和 chrony，不自动安装或切换服务。多个服务同时运行、当前服务不受支持或配置归属无法确认时，相关操作会说明原因。关闭当前服务后，本页面仍记住此次操作的服务，避免再次开启时换成另一个服务。

服务器表单可修改、清空现有地址或添加地址。chrony 保留原有 server/pool/peer 类型及其他参数，读取实际主配置和 include/confdir/sourcedir，网络动态来源只读。timesyncd 使用独立覆盖文件，检查最终读取顺序；系统默认备用来源在未修改时保持原样。

保存前分页显示完整差异，再检查候选配置；chrony 使用原生 `chronyd -p`，timesyncd 检查生成内容及合并顺序。检查进程在私有挂载空间查看候选文件，正式文件此时不变。全部版本确认后创建一份分组备份，再逐个保存；失败恢复先前已写文件，外部改动会阻止覆盖并报告冲突。恢复记录沿用“配置备份与恢复”入口。

服务原本运行时，保存后重启并重新读取状态；已停止时保持停止。配置保存成功、服务重启成功、系统时间实际同步分别报告，重启失败保留已保存配置及恢复记录。Tundra 自身的显示时区和时间来源不随这些操作更改。

普通验证：`cargo test --locked -p platform --lib`。真实写入验证使用 `scripts/tests/isolated-systemd.sh TEST_BINARY UID settings CHRONY_BUNDLE`，要求 root；脚本先创建独立的挂载、PID、网络和主机名空间，并使用私有 /etc、DBus、systemd 和备份目录。chrony 检查器只从官方包解压，不安装服务。测试服务使用不校时的占位进程，验证配置、服务启停及失败处理，不验证互联网授时精度。退出后清理专用 cgroup 和临时环境。

参考：[systemd 主机名接口](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.hostname1.xml)、[系统时间接口](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.timedate1.xml)、[chronyd 配置检查](https://chrony-project.org/doc/4.7/chronyd.html)。
