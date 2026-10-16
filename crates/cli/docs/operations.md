# Operations CLI / 运维命令

Use `tundra-cli <group> help` for exact syntax. In the built-in Command Line, prefix UX commands with `/`, for example `/services list`. System commands continue to run without that prefix.

使用 `tundra-cli <模块> help` 查看参数。在内建 Command Line 中使用 `/services list` 等带 `/` 的命令；系统命令仍直接输入。

| Group / 模块 | Queries / 查询 | Changes / 修改 |
| --- | --- | --- |
| `services` | `list`, `show`, `dependencies`, `config` | `start`, `stop`, `restart`, `reload`, `daemon-reload`, `enable`, `disable`, `create`, `create-instance` |
| `processes` | `list`, `show`, `files`, `ports`, `io`, `service` | `term`, `kill`, `stop`, `cont`, `nice` |
| `packages` | `list`, `show`, `search`, `updates`, `status`, `conflicts`, `check`, `sources` | `install`, `remove`, `upgrade`, `upgrade-all`, `refresh`, `repair-configure`, `repair-dependencies`, source drafts |
| `network` | `list`, `show`, `wifi-list`, `check`, `transaction-status` | `wifi-scan`, `wifi-connect`, `wifi-disconnect`, `wifi-forget`, `forget-saved-wifi`, `configure`, `disconnect`, `confirm` |
| `disks` | `list`, `show`, `health`, `inodes`, `scan` | `mount`, `unmount`, `automatic-mount` draft |
| `users` | `list`, `show`, `lookup`, `groups`, `shells`, `ssh-keys` | account, group, Shell, expiry and SSH-key commands |
| `system-config` | `read`, `diff`, `check`, `history`, `preview-restore` | `apply`, `permissions`, `restore`, `reload` |
| `operations` | `list`, `status` | `attach` reconnects to an existing task |
| `firewall` | `list`, `show`, `listeners` | `allow`, `remove` |
| `ssh` | `show`, `keys`, `status` | `configure`, `add-key`, `remove-key`, `confirm`, `cancel` |
| `backups` | `list`, `show`, `preview-restore` | `restore` |
| `scheduled-tasks` | `list`, `inspect`, `preview` | `create`, `pause`, `resume`, `run-now` |

The command and exit-code tables are bilingual; detailed behavior below is in Chinese. Use `tundra-cli <group> help` for English syntax.

查询支持 `--json`。文本列表第一列是可用于后续操作的准确 ID。JSON 模式下，标准输出只含结果，进度和原生工具输出写入标准错误。日志持续跟随使用 `logs follow --json` 输出 JSONL。

`users set-groups 用户 --groups 用户组` 修改补充组；`users primary-group 用户 --primary-group 用户组` 将主组改为已有的本地组。仍作为任何账户主组使用的组不能删除。

## Firewall, SSH, backups and schedules / 防火墙、SSH、备份与定时任务

```text
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
tundra-cli scheduled-tasks inspect TASK_ID --json
tundra-cli scheduled-tasks preview --name daily-job --command '/usr/bin/true' --schedule daily --json
tundra-cli scheduled-tasks create --name daily-job --command '/usr/bin/true' --schedule daily --yes --non-interactive
tundra-cli scheduled-tasks pause TASK_ID --yes
tundra-cli scheduled-tasks resume TASK_ID --yes
tundra-cli scheduled-tasks run-now TASK_ID --yes
```

防火墙只操作已有 UFW/firewalld 的放行规则。UFW 同时修改保存规则与运行规则，停用时只保存；firewalld 使用 `--zone`，`--duration runtime` 只立即生效，`permanent` 只保存并在重载或重启后使用，`both` 分别更新两份配置。不会自动安装、启用或切换后端；同时运行时拒绝写入。端口列表显示当前网络命名空间的 TCP 监听与未连接 UDP socket，以及 IPv4/IPv6 地址和能够读取的进程。

SSH `show`、`keys` 和 `configure` 可省略账户，此时使用启动程序的 Linux 账户。`configure` 的可选字段是 `--port`、`--password-authentication yes|no`、`--pubkey-authentication yes|no`、`--permit-root-login yes|prohibit-password|forced-commands-only|no` 和 `--open-firewall true|false`；未提供的字段保持界面读取的当前值。新增公钥使用 `add-key USER --public-key 'ssh-ed25519 ...'`，移除使用 `remove-key USER --fingerprint SHA256:...`。

SSH 修改等待最多 120 秒。保留原命令运行，从另一个禁用连接共享的真实新 SSH 连接登录，再运行 `ssh confirm 事务ID`；`ssh status 事务ID` 查询，`ssh cancel 事务ID --yes` 取消。超时或原操作取消会恢复旧设置。确认必须由实际新连接发出，不信任用户填写的环境变量。可选防火墙同步只新增新端口，不删除旧端口。操作中的事务编号写入标准错误，JSON 结果保留 `transaction_id`。

备份按修改操作分组，显示来源、文件、结果与差异；历史记录受系统权限保护，需要时进入授权。恢复前核对当前文件及受影响的运行状态，拒绝覆盖其他程序后来的修改。`--allow-unvalidated` 只用于明确接受没有可用配置检查器的恢复；检查失败仍会阻止恢复。旧的单文件配置备份也可在同一列表选择恢复。

定时任务统一列出 systemd 系统/用户 timer、当前用户 crontab、`/etc/crontab`、`/etc/cron.d` 和系统周期目录。`inspect` 返回同一任务的详情，包含命令、运行账户、上次结果、下次时间；cron 本身不保存原定时执行的退出结果，手动执行的结果单独显示。新任务先 `preview` 查看 service/timer 定义，再 `create` 保存；使用系统时区，错过的触发时间不会补跑。暂停只阻止之后触发，不停止已经运行的命令。

`firewall`、`ssh`、`backups` 与 `scheduled-tasks` 的只读查询可以加 `--authorize` 请求完整受保护信息。必须授权才能读取的防火墙/SSH/备份查询会自动进入同一授权流程。脚本可配合 `--authorization-fd`，缺少必要授权输入时沿用退出码 6。预览动作的原生文本保存在 JSON 的 `output` 数组，任务查询结果仍使用统一 `ManagementSnapshot` 格式。

## Confirmation and authorization / 确认与授权

`--yes` 表示确认本次修改；`--non-interactive` 禁止弹出输入问题。系统授权仍须满足。密码只能通过隐藏输入或指定描述符读取：`--password-fd FD` 用于 Wi-Fi/账户密码，`--authorization-fd FD` 用于 sudo 密码。密码不能写成命令行参数。每次读取只消费一行，内部复制描述符，保留原始句柄。

有效的正式运维命令同时携带这两个标志时，也满足 root 启动确认。Shell 和 REPL 仍保留原有交互确认。这两个标志不会允许受保护的账户操作，也不代替系统授权。

## Services and selected processes / 服务与选中进程

选中服务后可查看最近结果、退出原因和对应日志。`Requires/Wants/BindsTo` 表示依赖；`After/Before` 表示启动先后。反向依赖显示哪些服务需要当前服务。失败日志限定当前服务、启动批次和本次运行编号；编号缺失时使用最近启动时间。“查看日志”会把这些筛选条件带入日志页面。

`services reload 服务` 要求运行中的服务重载自身配置；`services daemon-reload` 要求 systemd 重新读取服务定义。配置编辑默认打开专属覆盖文件，模板实例使用实例自己的覆盖文件，新建服务使用独立单元文件。在编辑器中检查并保存后，再选择重新读取定义、启动或设为开机启用；创建时不会自动执行这些操作。

只为选中的进程补采集打开文件、实际持有的 TCP/UDP 端口、磁盘累计读写量和速率，以及所属 systemd 服务。采集前后均核对进程启动时间。进程退出、编号复用、缺少计数或无法确定端口所在网络时，都显示明确结果。文件与服务按钮可跳转文件管理器或对应服务。速率通过短时间内的两次读数计算，磁盘总量来自内核累计计数。

## Package recovery and sources / 软件包恢复与软件源

软件包状态检查实际占用。APT 锁文件仅仅存在不代表被占用。能确认时显示进程编号和名称；无法确定的 pacman 锁或过期 DNF 编号文件显示“占用者未知”。等待后重新检查，界面和 CLI 都不会删除锁或结束占用进程。

失败详情区分占用、下载、签名、依赖、空间不足和操作未完成。处理前先查看原始输出。APT 提供明确的 `repair-configure` 和 `repair-dependencies` 命令；`packages check` 执行后端检查。DNF/pacman 提供处理指引，不自动重建数据库或重跑事务。`packages conflicts` 列出配置候选，先比较当前文件与候选，再通过编辑器合并；保留候选原文件。

软件源读取 APT 传统格式和 deb822、DNF 仓库，以及 pacman 仓库与 Include 文件。新增、启用、禁用和移除先生成编辑器草稿，保留无关内容与注释。检查只解析候选，不刷新源，也不表示下载一定成功。新软件源要求签名检查。pacman Include 支持固定路径和文件名中的 `*`/`?`；不支持的目录模式明确提示使用编辑器处理。

软件包脚本模式须同时传入这两个标志，使用原生非交互选项，默认保留当前配置。APT 额外支持 `--config-policy replace`；DNF/pacman 保留各自的配置候选文件。Arch 安装和升级始终执行完整系统升级。程序不会自动删除锁文件、结束占用进程或重建数据库。

## Configuration drafts / 配置草稿

创建服务、修改软件源和设置开机挂载先生成候选文件。`--json` 返回 `path`、`content`、`validator` 和相关服务，此时尚未保存或启动服务。同一命令带 `--apply --expected-version VERSION` 才授权保存；版本从 `system-config read PATH` 取得。交互使用时确认修改，脚本另加 `--yes --non-interactive`。

```text
tundra-cli services create demo --program /usr/bin/sleep --arguments 30 --json
tundra-cli packages sources --json
tundra-cli system-config read /etc/ssh/sshd_config --json
tundra-cli system-config diff /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config check /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config apply /etc/ssh/sshd_config --input sshd.candidate --expected-version VERSION --yes
tundra-cli system-config reload /etc/ssh/sshd_config --service ssh.service --backup-id SAVED_BACKUP_ID --yes
tundra-cli system-config history /etc/ssh/sshd_config --json
tundra-cli system-config preview-restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --json
tundra-cli system-config restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --yes
```

检查失败会阻止保存。没有可用检查器时，需要显式选择 `--allow-unvalidated`。保存会保留受保护的恢复记录。重新加载失败时保留本次修改，可查看日志、继续编辑或恢复旧版本。保存和恢复均重新检查版本，不覆盖他人刚修改的文件。

`system-config check 文件` 检查当前内容；提供 `--input 候选文件` 时，在临时视图中检查候选，不提前替换正式文件。

systemd 文件的 `system-config reload` 默认重新读取服务定义并检查相关服务；增加 `--reload-service` 才重载运行中的服务。SSH 使用已识别的 SSH 服务重载，并要求传入保存时返回的 `document.backup_id`，以便重载失败时恢复原运行配置。两种操作都不会启动未运行的服务。

## Tasks and network confirmation / 后台任务与网络确认

默认最多等待 900 秒，可通过 `--wait 秒数` 修改。等待超时或连接断开时，返回未完成状态及已取得的任务 ID，系统助手继续执行。`operations attach ID` 重新连接同一任务，不重新执行；`operations status ID` 默认等待一秒。CLI 退出不会强杀已开始的软件包写入。

脚本网络配置返回 `awaiting_confirmation` 和 `transaction_id`。检查连接后，执行 `network confirm 事务ID --yes --non-interactive`。120 秒内未确认则恢复旧设置。Wi-Fi 自动核对所选网络和所需地址，互联网连接另行检查。

| Exit / 退出码 | Meaning / 含义 |
| --- | --- |
| 0 | Success / 成功 |
| 1 | Failure or partial change / 失败或部分完成 |
| 2 | Invalid arguments / 参数错误 |
| 3 | Permission denied / 无权限 |
| 4 | Unsupported or unavailable / 不支持或所需工具不可用 |
| 5 | Busy or version conflict / 占用或版本冲突 |
| 6 | Input required / 需要输入 |
| 7 | Unfinished or unknown / 未完成或结果未知 |
| 130 | Cancelled / 取消 |

后端提供原生工具退出码时，保留在结构化错误结果中。
