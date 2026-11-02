# 定时任务

定时任务页统一显示系统级和当前用户的 systemd timer、当前用户 crontab、`/etc/crontab` 与 `/etc/cron.d`。列表包含来源、执行用户、计划、状态、上次结果和下次时间。

## 新建任务

向导分为“做什么 → 何时执行 → 检查并创建”。

- 内置操作包括重启服务、更新本机软件包和运行脚本；高级模式可填写命令、工作目录和账户。
- 时间支持每天、每周和指定日期一次，显示系统时区及未来五次执行时间。
- 高级模式保留 `OnCalendar` 表达式，预览使用 `systemd-analyze calendar --iterations=5`。
- 创建前使用 `systemd-analyze verify` 检查 service/timer，再经 AA 和系统授权保存到 `/etc/systemd/system/` 并启用。
- 显示名称与内部 `tundra-scheduled-…` 单元编号分开，同名单元不会直接覆盖。

任务使用所选用户及主组，并设置 HOME、USER、LOGNAME 和 PATH。systemd 负责后续触发，关闭 Tundra 后任务继续运行。“错过后补执行一次”对应 `Persistent`，默认关闭。

每次执行按 systemd 执行编号写入 `/var/lib/tundraux3/scheduled-results/UID/history/`，保存开始、结束、结果和输出，可在“任务”页查看。

## 编辑与删除

所有已列出的任务都可编辑。简单每天、每周规则使用时间表单，复杂规则进入高级编辑。

- systemd timer 优先写覆盖配置。
- cron 只修改选中行，保留其他行、环境、注释和暂停标记。
- 能确认由用户创建的任务可以删除；系统预置或来源不明的任务提供停用和恢复。
- 删除前重新核对配置版本，运行中的任务需要先结束或明确取消。
- 删除不移除共享 service、执行历史或业务文件。

## 暂停、恢复和立即执行

多选操作处理全部勾选任务，只有全部目标都支持时才可执行。同一 cron 来源先按原版本核对，再一次保存和备份。

- 暂停 timer 停止以后触发并取消启动启用，正在执行的 service 继续运行。
- 恢复 timer 重新启用并启动；没有安装规则的 timer 只启动。
- 立即执行 timer 对应的 service，不修改其账户、环境或其他定义。
- 暂停 cron 添加 `# TUNDRAUX3-PAUSED `，恢复移除该标记，不把 cron 转成 timer。
- 立即执行 cron 创建独立临时 systemd service，使用原条目的账户、环境和百分号输入规则。

cron 通常不保存定时执行的退出码，因此列表显示“cron 未记录”；手动执行结果单独显示。

五字段计划和常见别名由 `croner` 计算，`CRON_TZ` 使用 `chrono-tz`。无法按本机 cron 可靠解释的扩展显示无法确定。

## 变更与恢复

写入前重新读取整个 cron 来源或 systemd 定义并核对版本。Cron 文件和新单元使用系统配置的安全保存，本机 crontab 通过原生 `crontab` 工具保存。

配置变化写入统一的 `scheduled-tasks` 备份历史，保存文件内容、属性、crontab 文本以及 timer 启用和活动状态。恢复前同时核对当前文件和 timer 状态，发现后续变化则停止。恢复 timer 状态不会停止已经运行的 service。

## 验证

```sh
cargo test --locked -p platform management::scheduled_tasks --lib
cargo test --locked -p platform --lib --no-run
sudo bash scripts/tests/isolated-systemd.sh /absolute/path/to/platform-test-binary "$(id -u)"
```

常规测试覆盖计划解析、环境、暂停恢复、并发修改和输出。隔离脚本创建私有挂载、PID、网络、cgroup、systemd 和 crontab，验证创建、立即执行、结果记录、恢复和失败路径。
