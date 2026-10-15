# 定时任务

定时任务页统一读取系统级 systemd timer、当前 Linux 用户的 systemd timer、当前用户的 crontab，以及 `/etc/crontab` 和 `/etc/cron.d`。列表显示来源、执行用户、计划、当前状态、上次结果和下次时间；缺少工具、没有权限或用户服务管理器不可用时，会显示具体原因。

显示文字从 `management-scheduled-tasks.ftl` 读取，后台线程和授权助手使用调用方的语言快照，包括自定义翻译。定时器状态、cron 表达式、环境变量和任务身份仍保留原生值。

## 新建任务

填写名称、Shell 命令、工作目录、计划和执行用户。默认使用启动 Tundra 的本机 Linux 用户，也可选择其他本机账户，包括 root。

计划使用 systemd 的 `OnCalendar` 写法，例如 `daily`、`Mon..Fri 09:00`、`*-*-* 00:00:00`。新任务使用系统时区，不接受在计划中另外指定时区。预览会显示完整 `.service` 和 `.timer` 内容，并通过 `systemd-analyze calendar --iterations=5` 和 `systemd-analyze verify` 检查。

“预览新任务”不写文件。“创建并启用”经过 AA 和 Linux 授权后，再显示并确认实际定义，然后保存到 `/etc/systemd/system/tundra-scheduled-名称.service` 和对应的 `.timer`，重新读取定义并启用 timer。已有同名单元不会被覆盖。

命令通过 `/bin/sh -c` 执行，使用所选用户及其主组，并设置该账户的 HOME、USER、LOGNAME 和常用系统 PATH。程序先切换到填写的绝对工作目录，再执行命令；目录消失或无法进入时不会执行命令，并记录失败。结果记录程序从固定可用的目录启动，避免工作目录变化让结果也无法保存。任务由系统 systemd 管理；关闭页面、退出 Tundra 或退出登录不会取消以后触发的任务。`Persistent=false` 表示停机或暂停期间错过的时间不会补跑。

新任务结束后，按执行用户写入 `/var/lib/tundraux3/scheduled-results/UID/` 下的结果文件，记录结束时间、systemd 结果和退出码。结果不会因为关闭 Tundra 或机器重启而丢失。记录程序也以任务用户运行，不把 cron 的自定义环境交给 root 程序。执行用户可以修改自己目录内的记录，这些记录用于日常状态查看。

## 暂停、恢复和立即执行

- 暂停 systemd timer 会停止以后触发，已启用的 timer 同时取消启动启用；正在执行的 service 会继续运行。恢复会重新启用并启动可启用的 timer；没有安装规则的 timer 只启动。
- “立即执行”直接启动 timer 对应的 service，保留该 service 原有的执行用户、环境和其他设置。systemd 已在运行同一个 service 时，不会重复启动另一份相同实例。
- 暂停 cron 只在选中行前添加 `# TUNDRAUX3-PAUSED `；恢复去掉该标记。其他行、注释、环境设置和换行方式保持原样。程序不会把已有 cron 任务转成 timer。
- “立即执行”cron 会创建独立的临时 systemd service，使用原条目的本机账户、主组、HOME 工作目录、SHELL 和前面的环境设置。系统有 cron/crond 的 PAM 配置时使用相应配置。标准 cron 的未转义 `%` 会把后半部分送给命令的标准输入，后续 `%` 变成换行，`\%` 作为普通百分号。临时服务在 Tundra 退出后继续执行。

Cron 本身通常不保存每次定时执行的退出码，因此原定时任务显示“cron 未记录”。手动执行的结束结果单独列在详情中，不会冒充原定时执行结果。

标准五字段计划及常见别名由 `croner 3.0.1` 计算下一次时间，`CRON_TZ` 使用 `chrono-tz` 解析。`@reboot`、随机时间、`L`、`W`、`#` 等扩展无法可靠按本机 cron 解释时，显示无法确定，不猜测时间。不同 cron 实现是否支持 `CRON_TZ` 仍由本机 cron 决定。`/etc/cron.d` 只读取通常可被 cron 接受的字母、数字、下划线和连字符文件名。

## 变更与恢复

写入前重新读取整个 cron 来源或 systemd 定义，核对用户看到的版本。发现其他程序修改后拒绝旧请求，要求刷新。Cron 文件及新单元复用系统配置编辑器的安全保存，保留已有文件的权限、所有者和扩展属性，在保存前再次核对并从同一目录替换；新文件不会覆盖同名文件。本机 crontab 通过原生 `crontab` 工具保存，保存前再次核对内容。外部程序仍可能在最后一次核对后写入；原生 crontab 没有通用的“版本相同才保存”接口。

每次配置变化写入统一的 `scheduled-tasks` 备份历史。文件备份保存原内容和属性，原生 crontab 备份保存修改前后文本，timer 备份保存启用和活动状态。恢复前同时核对目前文件、crontab 和 timer 状态；出现后续变化会拒绝覆盖。恢复 timer 状态不会停止它触发的正在执行的 service。

## 验证

在本机 WSL 运行：

```sh
cargo test --locked -p platform management::scheduled_tasks --lib
```

常规测试使用临时文件和只读原生校验，覆盖 cron 环境与执行用户、暂停恢复原文、并发修改拒绝、五字段及别名、日与星期的 cron 规则、CRON_TZ、无法计算的扩展、百分号输入和命令参数。原生 `systemd-analyze` 检查生成的两个单元及候选时间。

[隔离 systemd 测试脚本](../../../scripts/tests/isolated-systemd.sh)另建独立的挂载、PID、网络和 cgroup 环境，隐藏宿主服务定义、`/run`、`/var/lib` 和 crontab 的 `/var/spool`，启动只包含测试单元的 systemd。其 ignored 用例验证创建、立即执行、结果文件、暂停、恢复、撤销恢复和工作目录消失后的失败记录，也验证原生 crontab 的暂停及恢复。

先以普通用户在仓库根目录编译 platform 测试，再在 WSL root 下把 Cargo 显示的测试二进制绝对路径与原用户 UID 传给脚本：

```sh
cargo test --locked -p platform --lib --no-run
sudo bash scripts/tests/isolated-systemd.sh /absolute/path/to/platform-test-binary "$(id -u)"
```

脚本结束时清理专用 cgroup 和输出目录；不修改宿主服务或 crontab。实际 sudo 认证窗口、真实设备重启后的执行仍须单独验收。
