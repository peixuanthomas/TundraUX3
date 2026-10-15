# 防火墙与监听端口

`management::firewall` 查询和修改现有 UFW 或运行中的 firewalld。只新增、删除放行规则，不安装、启用或切换防火墙；其他后端只显示“暂不支持”。同时检测到 UFW 和 firewalld 运行时，保留查询并禁止写入。

## 监听端口

`query.scope = "listeners"` 读取当前网络命名空间的 `/proc/net/tcp`、`tcp6`、`udp`、`udp6`。TCP 只列监听状态；UDP 列未连接但已绑定的地址和端口。IPv4、IPv6、协议、完整地址、端口和进程分开显示。

`listening_ports` 将 socket inode（内核给 socket 的编号）与 `/proc/PID/fd` 对照，并在读取前后核对进程的 PID 和启动时间。权限不足、进程退出或其他命名空间中的 socket 不会显示成“端口无人使用”。经 AA 授权可以读取受保护的进程信息；不会修改进程。

## 原生规则

- UFW 使用 `status numbered` 查询运行规则；停用时使用 `show added` 查询保存规则。新增放行规则使用原生 `allow in proto ... from ... to any port ...`，端口范围转换为 UFW 的冒号格式。UFW 的保存规则和即时规则一起修改，停用时只保存且不启用。编号只用于显示和核对；删除运行规则使用核对后的完整原生参数及地址族，不能可靠解析时拒绝删除。停用时使用原生保存规则的完整参数；SSH 规则另核对事务注释。
- firewalld 使用默认 zone、活动 zone 和两次 `--list-all-zones` 分别读取 runtime、permanent 配置，避免为每个 zone 重复启动客户端。新增规则使用 rich rule，支持来源 IP/网段、TCP/UDP 和端口范围。`duration=runtime` 只立即应用，重载/重启后消失；`permanent` 只保存，重载/重启后使用；`both` 分别修改两份配置，不执行全局重载。已有服务、端口、来源端口和 rich 放行规则可以删除，其余规则与设置只查看。

所有系统工具都由固定目录寻找，以固定参数数组运行；不拼接 shell。来源地址、端口范围、协议和 zone 都先检查。写入前和备份后重新读取规则，与确认时的完整状态比较，状态已变则拒绝旧请求。本应用的写操作使用同一个文件锁排队；外部原生命令不遵守这个锁，仍可能在最后一次核对与原生命令之间修改规则。

普通查询先用现有 DBus 库的 `NameHasOwner` 检查 firewalld 已有连接，不激活缺失服务；只有确认它已运行才调用 `firewall-cmd --state`。DBus 查询使用已有 30 秒超时，查询失败时停止判断和写入。

动作名：`show_listening_ports`、`show_firewall_rules` 为页面切换；`inspect_firewall` 为授权只读查询；`add_rule`、`remove_rule` 为写入。新增字段为 `protocol`、`port`、`source`，firewalld 另有 `zone`、`duration`。`source` 为空表示任意来源。

## 备份和恢复

写入前调用共享 `config_backups::begin("firewall", ...)`，保留 UFW 的 `user.rules`、`user6.rules`、`ufw.conf`、`/etc/default/ufw`，或受影响的 firewalld zone XML 及原生工具生成的 `.xml.old`。操作记录另存原生规则修改前后状态，以及原来没有自定义覆盖文件的 zone；成功和失败都会结束备份记录。

共享恢复先持有 `lock_rules`，调用 `restore_preflight` 核对当前规则并检查备份文件。`restore_backup(backup_id, payload, ...)` 先建立一条同时包含当前文件与运行规则的逆向备份，结束同一条记录；这次恢复也能完整撤销。UFW 先恢复文件，仅在原本启用且仍启用时调用 `ufw reload`。firewalld 先撤销本次精确规则；原来自定义 XML 不存在时，用原生 `--load-zone-defaults` 同步保存缓存，再恢复 XML 和 `.old` 的原件或原不存在状态。恢复不会全局重载、不覆盖其他临时规则。当前受影响规则或配置文件已被外部修改时拒绝恢复。

SSH 同步先调用 `prepare_ssh_ports(transaction, ports, flag)`，把返回的 `OwnedPortRule` 写入 SSH 恢复记录，通过 `ssh_backup_paths` 将 UFW 文件并入同一记录，再调用 `apply_ssh_ports_unrecorded`；撤回使用 `rollback_ssh_ports_unrecorded`，由 SSH 外层结束整条记录，不产生独立的防火墙历史。`present_ssh_rules` 只返回当前仍在且与句柄精确匹配的规则，供整次 SSH 恢复使用。

只新增缺失 TCP 端口，不删除旧 SSH 端口或原有规则。失败时只删除本次新增规则：UFW 以事务注释识别；firewalld 以事务生成的 priority、地址族、zone 和保存范围识别。firewalld 不支持规则备注，外部程序若并发创建完全相同的规则，原生接口无法区分归属；准备和应用时分别检查规则不存在，恢复仅处理记录中的精确规则。

SSH 开机恢复使用三个 `_boot` 接口，不等待尚未启动的 DBus。先经 PID1 查询 firewalld 服务状态；已运行时使用正常原生接口，尚未运行时只用官方 `firewall-offline-cmd` 撤销或重加本次永久规则，跨重启的临时规则视为已消失。停用的 UFW 从 `user.rules`、`user6.rules` 的原生 tuple 只读核对地址族、端口和事务注释，再用官方 UFW 命令精确增删，不直接覆盖保存文件。

UFW 配置为 `ENABLED=yes` 但服务尚未加载规则时，官方命令会先保存修改，再因缺少内核规则而重载失败。只有这一开机状态、已知的 `ERROR: problem running`、保存规则精确读回符合预期且 UFW 仍停用时，才将保存恢复视为成功；其他错误继续报告失败，不启用防火墙。

## 验证

Linux 测试须在本机 Linux/WSL 运行：

```sh
cargo test --locked -p platform management::firewall --lib
cargo test --locked -p platform management::listening_ports --lib
```

测试覆盖原生查询输出、IPv6 来源、即时/保存区别、编号过期、双后端拒写、参数限制、停用 UFW、恢复冲突和 SSH 规则归属；监听测试会打开本机临时 TCP socket 并核对进程。普通测试不修改真实防火墙。

`tests/firewall_isolated.rs` 中两个 ignored 用例使用真实 Ubuntu 官方 UFW/firewalld，只能由 `scripts/tests/firewall-isolated.sh TEST_BINARY UNPACKED_ROOT` 启动。先将官方 deb 包下载、解压到独立目录，不安装主机服务；脚本创建私有 mount/network/pid/run/varlib 和 DBus，阻止全局内核模块加载，并比较宿主 nftables 规则前后保持相同。用例验证 UFW 启用/停用增删、firewalld 两份配置、原生重载、完整恢复及撤销恢复，每次恢复只产生一条逆向记录。它们不代替实际机器断网、系统重新启动及不同发行版验收。

同一脚本的 `PLATFORM_UNIT_BINARY UNPACKED_ROOT ssh SSH_UNPACKED_ROOT` 参数运行真实 SSH 与 UFW 组合回退测试。SSH 包同样只解压、不安装；私有 `/etc/shadow` 只生成测试账户内容，不读取宿主密码文件。脚本在运行前核对二进制包含指定用例，避免选错二进制时把零用例当作通过。

`PLATFORM_UNIT_BINARY UNPACKED_ROOT firewalld-boot` 运行无 DBus、无 firewalld 守护进程的真实离线用例，验证只增删本次永久规则、保留原有端口，且不会恢复已跨重启消失的临时规则。
