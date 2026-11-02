# 防火墙与监听端口

`management::firewall` 查询和修改现有 UFW 或运行中的 firewalld，提供放行规则新增和删除。两个后端同时运行时保留只读查询，避免修改来源不明确。

## 监听端口

`query.scope = "listeners"` 读取当前网络命名空间的 `/proc/net/tcp`、`tcp6`、`udp` 和 `udp6`。TCP 显示监听状态，UDP 显示已绑定且未连接的地址。

`listening_ports` 将 socket inode 与 `/proc/PID/fd` 对照，并在读取前后核对 PID 和启动时间。权限不足、进程退出和其他命名空间分别报告，AA 可用于读取受保护的进程信息。

## 原生规则

### UFW

- 运行中使用 `status numbered`，停用时使用 `show added` 读取保存规则。
- 新增使用原生 `allow in proto ... from ... to any port ...`。
- 运行规则与保存规则一起修改，停用时只保存，不启用防火墙。
- 删除前核对完整原生参数、地址族和规则身份，编号只用于显示与核对。

### firewalld

- 使用默认 zone、活动 zone 和 `--list-all-zones` 分别读取 runtime 与 permanent 配置。
- 新增 rich rule，支持来源 IP/网段、TCP/UDP 和端口范围。
- `runtime` 只立即生效，`permanent` 只保存，`both` 分别修改两份配置，不执行全局重载。
- 已有服务、端口、来源端口和 rich 放行规则可删除，其他规则与设置只读。

所有工具以固定路径和参数数组执行。写入前和备份后重新读取完整规则状态，发现外部变化则拒绝旧请求；本应用的写操作通过文件锁排队。

动作包括 `show_listening_ports`、`show_firewall_rules`、`inspect_firewall`、`add_rule` 和 `remove_rule`。新增字段为 `protocol`、`port`、`source`，firewalld 另有 `zone` 和 `duration`。

## 备份与恢复

写入前通过 `config_backups::begin("firewall", ...)` 保存 UFW 配置文件或受影响的 firewalld zone XML，同时记录原生规则前后状态。

恢复前核对当前规则和备份文件，并先建立当前状态的逆向备份，因此恢复操作本身也可撤销。

- UFW 恢复文件后，只在原本且目前仍启用时执行 `ufw reload`。
- firewalld 精确撤销本次规则，并恢复 zone XML 与原生 `.xml.old`；不执行全局重载。
- 当前规则或文件已被外部修改时停止恢复。

SSH 端口规则通过 `prepare_ssh_ports` 和对应恢复接口并入 SSH 的同一操作记录。只添加缺失的新 TCP 端口，旧端口和原有规则保持不变，回退只删除本次新增规则。

开机恢复在 D-Bus 和 firewalld 尚未运行时，使用官方 `firewall-offline-cmd` 处理永久规则；UFW 使用官方命令按原生规则身份增删，不直接覆盖规则文件。

## 验证

```sh
cargo test --locked -p platform management::firewall --lib
cargo test --locked -p platform management::listening_ports --lib
```

普通测试使用离线数据和临时 socket，不修改真实防火墙。

真实 UFW/firewalld 用例通过官方包解压和私有 mount/network/PID/D-Bus 环境运行：

```sh
scripts/tests/firewall-isolated.sh TEST_BINARY UNPACKED_ROOT
scripts/tests/firewall-isolated.sh PLATFORM_UNIT_BINARY UNPACKED_ROOT ssh SSH_UNPACKED_ROOT
scripts/tests/firewall-isolated.sh PLATFORM_UNIT_BINARY UNPACKED_ROOT firewalld-boot
```

这些用例覆盖规则增删、runtime/permanent、完整恢复、SSH 组合回退和无守护进程的开机恢复，并核对宿主 nftables 规则未改变。
