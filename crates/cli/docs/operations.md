# 运维 CLI

使用 `tundra-cli <模块> help` 查看准确参数。内建 Command Line 中的 UX 命令加 `/`，例如 `/services list`；系统命令仍直接输入。

## 命令分组

| 模块 | 查询 | 修改 |
| --- | --- | --- |
| `services` | `list`, `show`, `dependencies`, `config` | `start`, `stop`, `restart`, `reload`, `daemon-reload`, `enable`, `disable`, `create`, `create-instance` |
| `processes` | `list`, `show`, `files`, `ports`, `io`, `service` | `term`, `kill`, `stop`, `cont`, `nice` |
| `packages` | `list`, `show`, `search`, `updates`, `status`, `conflicts`, `check`, `sources` | `install`, `remove`, `upgrade`, `upgrade-all`, `refresh`, 修复与软件源草稿 |
| `network` | `list`, `show`, `wifi-list`, `check`, `transaction-status` | Wi-Fi、`configure`, `disconnect`, `confirm` |
| `disks` | `list`, `show`, `health`, `inodes`, `scan` | `mount`, `unmount`, `automatic-mount` 草稿 |
| `users` | `list`, `show`, `lookup`, `groups`, `shells`, `ssh-keys` | 账户、用户组、Shell、有效期和公钥 |
| `system-config` | `read`, `diff`, `check`, `history`, `preview-restore` | `apply`, `permissions`, `restore`, `reload` |
| `operations` | `list`, `status` | `attach` 重连已有任务 |
| `firewall` | `list`, `show`, `listeners` | `allow`, `remove` |
| `ssh` | `show`, `keys`, `status` | `configure`, `add-key`, `remove-key`, `confirm`, `cancel` |
| `backups` | `list`, `show`, `preview-restore` | `restore` |
| `scheduled-tasks` | `list`, `inspect`, `preview` | `create`, `pause`, `resume`, `run-now` |

查询支持 `--json`，标准输出只包含结果，进度和原生工具输出写入标准错误。文本列表第一列是后续操作使用的准确 ID，日志跟随的 JSON 模式输出 JSONL。

## Confirmation and authorization / 确认与授权

- `--yes` 确认本次修改，`--non-interactive` 禁止交互提问；系统授权仍需满足。
- `--password-fd FD` 用于 Wi-Fi 或账户密码，`--authorization-fd FD` 用于 sudo 密码。每次只读取一行，密码不放在命令行参数中。
- 正式运维命令同时使用 `--yes --non-interactive` 时，也满足 root 启动确认。
- 受保护的只读查询可加 `--authorize`；必须授权才能读取的查询会进入同一授权流程。

## 防火墙、SSH、备份与计划任务

```sh
tundra-cli firewall listeners --json
tundra-cli firewall list --json
tundra-cli firewall allow --port 8000-8010 --protocol tcp --source 192.0.2.0/24 --yes
tundra-cli firewall allow --port 8443 --zone public --duration both --yes
tundra-cli firewall remove RULE_ID --yes

tundra-cli ssh show alice --json
tundra-cli ssh configure alice --port 2222 --open-firewall true --yes
tundra-cli ssh confirm TRANSACTION_ID --yes --non-interactive

tundra-cli backups list --json
tundra-cli backups preview-restore BACKUP_ID --json
tundra-cli backups restore BACKUP_ID --allow-unvalidated --yes

tundra-cli scheduled-tasks list --json
tundra-cli scheduled-tasks preview --name daily-job --command '/usr/bin/true' --schedule daily --json
tundra-cli scheduled-tasks create --name daily-job --command '/usr/bin/true' --schedule daily --yes --non-interactive
tundra-cli scheduled-tasks pause TASK_ID --yes
tundra-cli scheduled-tasks resume TASK_ID --yes
tundra-cli scheduled-tasks run-now TASK_ID --yes
```

防火墙操作现有 UFW/firewalld 的放行规则。firewalld 的 `--duration` 支持 `runtime`、`permanent` 和 `both`，两个后端同时运行时保持只读。

SSH `show`、`keys` 和 `configure` 可省略账户，默认使用启动程序的 Linux 账户。`configure` 支持端口、密码认证、公钥认证、root 登录和 `--open-firewall`。未提供的字段保持当前值。

SSH 修改最多等待 120 秒。从另一个禁用连接共享的真实新 SSH 连接运行 `ssh confirm`；`ssh status` 查询，`ssh cancel` 取消。超时或取消恢复旧设置，事务编号通过标准错误和 JSON `transaction_id` 提供。

备份按操作分组，恢复前核对当前文件与运行状态。`--allow-unvalidated` 只表示接受没有可用检查器，检查失败仍会阻止恢复。

计划任务统一列出 systemd timer 与 cron。先 `preview` 查看定义，再 `create` 保存；暂停只阻止以后触发，不停止已经运行的命令。

## 服务与进程

服务详情包括最近结果、退出原因、依赖和日志。`services reload` 重载运行中的服务，`daemon-reload` 让 systemd 重新读取定义。创建服务或修改配置后，再显式选择加载、启动或启用。

进程详情可补充打开文件、TCP/UDP 端口、磁盘累计读写和速率、所属服务。采集前后核对 PID 和启动时间，进程退出或编号复用时返回对应状态。

## 软件包恢复与软件源

状态检查区分锁占用、下载、签名、依赖、空间和未完成事务。APT 提供 `repair-configure` 和 `repair-dependencies`，其他后端显示处理指引，锁文件和占用进程由用户根据原生输出处理。

`packages conflicts` 列出配置候选，比较后通过编辑器合并。软件源支持 APT 传统格式与 deb822、DNF 和 pacman，新增、启停用和移除先生成草稿。

脚本软件包操作使用 `--yes --non-interactive` 和原生非交互选项，默认保留当前配置；APT 可通过 `--config-policy replace` 明确替换。Arch 安装与升级始终包含完整系统升级。

## 配置草稿与保存

创建服务、修改软件源和设置开机挂载先返回包含 `path`、`content`、`validator` 的草稿。添加 `--apply --expected-version VERSION` 才进入保存。

```sh
tundra-cli services create demo --program /usr/bin/sleep --arguments 30 --json
tundra-cli system-config read /etc/ssh/sshd_config --json
tundra-cli system-config diff /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config check /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config apply /etc/ssh/sshd_config --input sshd.candidate --expected-version VERSION --yes
tundra-cli system-config reload /etc/ssh/sshd_config --service ssh.service --backup-id SAVED_BACKUP_ID --yes
tundra-cli system-config history /etc/ssh/sshd_config --json
tundra-cli system-config preview-restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --json
tundra-cli system-config restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --yes
```

检查失败阻止保存，没有检查器时显式使用 `--allow-unvalidated`。systemd 的 `reload` 默认重新读取定义，`--reload-service` 同时重载服务；SSH 需要保存时返回的 `document.backup_id`。

## 后台任务与网络确认

默认最多等待 900 秒，可通过 `--wait 秒数` 调整。超时或断线返回未完成状态和任务 ID，助手继续执行；`operations attach ID` 重连，`operations status ID` 默认等待一秒。

网络配置返回 `awaiting_confirmation` 和 `transaction_id` 后，检查连接并执行：

```sh
tundra-cli network confirm TRANSACTION_ID --yes --non-interactive
```

120 秒内未确认则恢复旧设置。

## 退出码

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功 |
| 1 | 失败或部分完成 |
| 2 | 参数错误 |
| 3 | 无权限 |
| 4 | 不支持或所需工具不可用 |
| 5 | 占用或版本冲突 |
| 6 | 需要输入 |
| 7 | 未完成或结果未知 |
| 130 | 取消 |

原生工具退出码可用时保留在结构化错误结果中。
