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

Queries support `--json`. Text lists include the exact row ID before the displayed columns, so the same ID can be used for a later operation. In JSON mode, standard output contains the result object; progress and native tool output go to standard error. Log following uses the existing `logs follow --json` JSONL stream.

查询支持 `--json`。文本列表第一列是可用于后续操作的准确 ID。JSON 模式下，标准输出只含结果，进度和原生工具输出写入标准错误。日志持续跟随使用 `logs follow --json` 输出 JSONL。

`users set-groups USER --groups GROUPS` edits supplementary memberships. `users primary-group USER --primary-group GROUP` changes the primary group to an existing local group. A group still used as anyone's primary group cannot be removed.

`users set-groups 用户 --groups 用户组` 修改补充组；`users primary-group 用户 --primary-group 用户组` 将主组改为已有的本地组。仍作为任何账户主组使用的组不能删除。

## Confirmation and authorization / 确认与授权

`--yes` confirms the requested change. `--non-interactive` prevents input prompts. System authorization remains required. Passwords are read from a hidden terminal prompt or a descriptor: `--password-fd FD` for a Wi-Fi/account password and `--authorization-fd FD` for sudo. A password cannot be supplied as a command-line value. Each descriptor read consumes one line; the descriptor is duplicated and its original handle is retained.

`--yes` 表示确认本次修改；`--non-interactive` 禁止弹出输入问题。系统授权仍须满足。密码只能通过隐藏输入或指定描述符读取：`--password-fd FD` 用于 Wi-Fi/账户密码，`--authorization-fd FD` 用于 sudo 密码。密码不能写成命令行参数。每次读取只消费一行，保留原始描述符。

A valid operations command carrying both flags also satisfies the root startup confirmation. Shell and REPL startup retain their existing interactive confirmation. These flags never permit a forbidden account operation or replace authorization.

有效的正式运维命令同时携带这两个标志时，也满足 root 启动确认。Shell 和 REPL 仍保留原有交互确认。这两个标志不会允许受保护的账户操作，也不代替系统授权。

## Services and selected processes / 服务与选中进程

Select a service to inspect its last result, exit reason and recent logs. Required services and ordering are listed separately: `Requires/Wants/BindsTo` describe dependencies, and `After/Before` describe order. The reverse dependency fields show services depending on the selection. Failure logs use the selected service, current boot and its invocation ID when available, otherwise its last start time. The log action carries these filters into the log viewer.

选中服务后可查看最近结果、退出原因和对应日志。`Requires/Wants/BindsTo` 表示依赖；`After/Before` 表示启动先后。反向依赖显示哪些服务需要当前服务。失败日志限定当前服务、启动批次和本次运行编号；编号缺失时使用最近启动时间。“查看日志”会把这些筛选条件带入日志页面。

`services reload UNIT` asks the running service to reload its own configuration. `services daemon-reload` asks systemd to reread service definitions. Service configuration opens a dedicated override file; creating a template instance uses an instance-specific override. Creating a new service produces its own unit file. Review and save the draft in the editor, then choose whether to reread definitions, start it or enable startup. Creation does none of those automatically.

`services reload 服务` 要求运行中的服务重载自身配置；`services daemon-reload` 要求 systemd 重新读取服务定义。配置编辑默认打开专属覆盖文件，模板实例使用实例自己的覆盖文件，新建服务使用独立单元文件。在编辑器中检查并保存后，再选择重新读取定义、启动或设为开机启用；创建时不会自动执行这些操作。

Process details are collected only for the selected PID. They show real open descriptor paths, TCP/UDP sockets owned by those descriptors, disk byte totals and measured rates, and the innermost systemd service from the process's control-group path. The PID's start time is checked before and after collection. Exit, reuse, unavailable counters and unknown socket namespaces are reported explicitly. File and service actions open the file manager or the corresponding service. CPU and disk rates use a short second sample; disk totals are cumulative kernel counters.

只为选中的进程补采集打开文件、实际持有的 TCP/UDP 端口、磁盘累计读写量和速率，以及所属 systemd 服务。采集前后均核对进程启动时间。进程退出、编号复用、缺少计数或无法确定端口所在网络时，都显示明确结果。文件与服务按钮可跳转文件管理器或对应服务。速率通过短时间内的两次读数计算，磁盘总量来自内核累计计数。

## Package recovery and sources / 软件包恢复与软件源

Package status inspects live locks. APT's existing lock file alone does not indicate occupancy. A confirmed owner is shown by PID and name; an unresolved pacman lock or stale DNF PID file is shown as owner unknown. Wait and check again. The UI and CLI do not delete locks or terminate owners.

软件包状态检查实际占用。APT 锁文件仅仅存在不代表被占用。能确认时显示进程编号和名称；无法确定的 pacman 锁或过期 DNF 编号文件显示“占用者未知”。等待后重新检查，界面和 CLI 都不会删除锁或结束占用进程。

Failure details distinguish occupancy, downloads, signatures, dependencies, insufficient space and an incomplete operation. Review native output before any repair. APT exposes explicit `repair-configure` and `repair-dependencies` commands. `packages check` runs the backend's read-only check; DNF and pacman provide guidance without automatic database rebuilds or transaction retries. Candidate files remain available under `packages conflicts`: compare the current file with `.dpkg-*`, `.rpmnew/.rpmsave` or `.pacnew/.pacsave`, then merge through the editor. The candidate is retained.

失败详情区分占用、下载、签名、依赖、空间不足和操作未完成。处理前先查看原始输出。APT 提供明确的 `repair-configure` 和 `repair-dependencies` 命令；`packages check` 执行后端检查。DNF/pacman 提供处理指引，不自动重建数据库或重跑事务。`packages conflicts` 列出配置候选，先比较当前文件与候选，再通过编辑器合并；保留候选原文件。

Source management reads APT traditional and deb822 files, DNF repositories, and pacman repositories with included files. Add, enable, disable and remove produce an editor draft preserving unrelated content and comments. Checks parse the candidate locally and do not refresh repositories or claim downloads will succeed. New sources require signature verification. Pacman Include expansion supports literal paths and `*`/`?` in the file name; unsupported directory patterns are reported for manual editing.

软件源读取 APT 传统格式和 deb822、DNF 仓库，以及 pacman 仓库与 Include 文件。新增、启用、禁用和移除先生成编辑器草稿，保留无关内容与注释。检查只解析候选，不刷新源，也不表示下载一定成功。新软件源要求签名检查。pacman Include 支持固定路径和文件名中的 `*`/`?`；不支持的目录模式明确提示使用编辑器处理。

Package script mode requires both flags and uses the manager's native noninteractive options. It keeps current configuration files. APT additionally supports `--config-policy replace`; DNF and pacman retain their native `.rpmnew/.rpmsave` and `.pacnew/.pacsave` candidates. Arch installs and upgrades always perform the full system upgrade. No task deletes a package lock, kills its owner, or rebuilds a database automatically.

软件包脚本模式须同时传入这两个标志，使用原生非交互选项，默认保留当前配置。APT 额外支持 `--config-policy replace`；DNF/pacman 保留各自的配置候选文件。Arch 安装和升级始终执行完整系统升级。程序不会自动删除锁文件、结束占用进程或重建数据库。

## Configuration drafts / 配置草稿

Service creation, software-source changes and startup mounting produce a candidate file first. `--json` exposes its `path`, `content`, `validator` and related service. They do not write a configuration file or start a service. To save from the same command, explicitly add `--apply --expected-version VERSION`; the version comes from `system-config read PATH`. Interactive use asks for confirmation; scripts also need `--yes --non-interactive`.

创建服务、修改软件源和设置开机挂载先生成候选文件。`--json` 返回路径、内容、检查器和相关服务，此时尚未保存或启动服务。同一命令带 `--apply --expected-version VERSION` 才授权保存；版本从 `system-config read PATH` 取得。交互使用时确认修改，脚本另加 `--yes --non-interactive`。

```text
tundra-cli services create demo --program /usr/bin/sleep --arguments 30 --json
tundra-cli packages sources --json
tundra-cli system-config read /etc/ssh/sshd_config --json
tundra-cli system-config diff /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config check /etc/ssh/sshd_config --input sshd.candidate
tundra-cli system-config apply /etc/ssh/sshd_config --input sshd.candidate --expected-version VERSION --yes
tundra-cli system-config reload /etc/ssh/sshd_config --service ssh.service --yes
tundra-cli system-config history /etc/ssh/sshd_config --json
tundra-cli system-config preview-restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --json
tundra-cli system-config restore /etc/ssh/sshd_config --backup-id ID --expected-version VERSION --yes
```

Check errors prevent a save. Unsupported checks require the explicit `--allow-unvalidated` choice. Saving keeps a protected recovery record. A failed reload keeps the saved change; inspect the logs, edit it again or restore an older version. Apply and restore recheck the supplied version; they do not overwrite a file changed by someone else.

检查失败会阻止保存。没有可用检查器时，需要显式选择 `--allow-unvalidated`。保存会保留受保护的恢复记录。重新加载失败时保留本次修改，可查看日志、继续编辑或恢复旧版本。保存和恢复均重新检查版本，不覆盖他人刚修改的文件。

`system-config check FILE` checks the current content. With `--input CANDIDATE`, it checks the candidate in the temporary view without replacing the actual file.

`system-config check 文件` 检查当前内容；提供 `--input 候选文件` 时，在临时视图中检查候选，不提前替换正式文件。

For systemd files, `system-config reload` rereads definitions and checks the selected service. Add `--reload-service` to also reload the running service. SSH reload uses the recognized SSH service. Neither operation starts an inactive service.

systemd 文件的 `system-config reload` 默认重新读取服务定义并检查相关服务；增加 `--reload-service` 才重载运行中的服务。SSH 使用已识别的 SSH 服务重载，两种操作都不会启动未运行的服务。

## Tasks and network confirmation / 后台任务与网络确认

Task waiting defaults to 900 seconds; set `--wait SECONDS` to change it. Timeout or disconnection returns an unfinished result and the operation ID when available. The helper continues the system operation. `operations attach ID` reconnects without starting it again. `operations status ID` waits one second by default. An active package write is never killed when the CLI leaves.

默认最多等待 900 秒，可通过 `--wait 秒数` 修改。等待超时或连接断开时，返回未完成状态及已取得的任务 ID，系统助手继续执行。`operations attach ID` 重新连接同一任务，不重新执行；`operations status ID` 默认等待一秒。CLI 退出不会强杀已开始的软件包写入。

Network configuration in script mode returns `awaiting_confirmation` and `transaction_id`. Check connectivity, then run `network confirm TRANSACTION --yes --non-interactive`. An unconfirmed change restores the previous settings after 120 seconds. Wi-Fi verifies the selected network and required addresses automatically; Internet reachability is checked separately.

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

Native tool exit codes are retained in the structured problem when the backend reports them.

后端提供原生工具退出码时，保留在结构化错误结果中。
