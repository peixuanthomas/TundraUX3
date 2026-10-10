# 系统配置文件 / System configuration files

系统配置仍在文本编辑器中编辑。服务覆盖配置、软件源和开机挂载配置会打开同一个编辑器，返回时保留原页面。

## 读取、预览和检查

读取时记录文件内容版本、所有者、用户组和权限。保存前显示内容与属性差异，检查候选文件，再请求系统授权。候选内容只出现在助手的私有连接和临时检查目录，不写入普通运行日志或普通编辑器草稿。检查失败阻止保存；工具缺失或没有检查器时显示“未检查”，必须明确选择继续保存。

Linux 检查使用单独进程的私有挂载空间。候选文件覆盖在该进程看见的临时目录中，正式文件不变。SSH 从主配置执行 `sshd -t -ddd`，包括主配置引用的文件和主机密钥。编辑片段时，还必须从 OpenSSH 的解析记录确认主配置实际读到了该片段；仅匹配 Include 模式但没有读取片段不算通过。systemd 使用 `systemd-analyze verify`，保留原服务和其他覆盖配置；用户服务使用 `--user` 和文件对应的 `SYSTEMD_UNIT_PATH`。fstab 使用 `findmnt --verify`，软件源使用各发行版的解析检查。配置有效与网络连接、挂载或服务运行成功分别报告。

## 保存与恢复

保存通过原子替换完成，保留所有者、用户组、权限、ACL 和扩展属性。新目录与新文件继承最近的已有父目录所有者，避免授权助手把用户服务文件变成 root 所有。文件路径逐层拒绝符号链接。备份完成后再次核对内容版本和父目录；外部改动返回差异检查。保留属性失败则停止写入。

每次保存先在 `/var/lib/tundraux3/config-backups` 保存恢复记录。目录为 `0700`，记录为 `0600`。记录包含原内容和属性，只有助手可读；不写入普通日志。恢复也先预览差异、检查当前版本和检查候选配置。恢复“原先不存在”会删除本次创建的配置文件，并留下这次删除前的恢复记录。

## 加载与命令行

默认仅保存。SSH 的“重新加载”只允许固定的 SSH 服务。systemd 默认重新读取服务定义；显式选择 `reload_service=true` 才再重新加载该服务。完成后查询服务状态。加载失败、查询失败或取消等待都保留已保存内容，可查看日志、继续编辑或手动恢复。

外部命令使用 `tundra-cli system-config read|diff|check|apply|permissions|history|restore|reload …`。脚本保存必须提供 `--expected-version`；未验证保存必须显式指定 `--allow-unvalidated`。系统授权不会被 `--yes` 替代。内建 Command Line 使用 `/system-config …`。

## 验证

普通自动测试覆盖版本冲突、保存期间的外部改动、父目录替换、取消或拒绝后的原文件、备份权限、ACL 与扩展属性、恢复原先缺失的文件，以及 SSH 片段解析证据。真实检查器测试默认忽略，须在私有 root 挂载空间运行；不修改真实 `/etc`。

```sh
# 用系统中已有的 sshd，或仅解压 Ubuntu openssh-server 包到私有目录。
# TUNDRA_TEST_CHECKER_LIBRARIES 可指向同一私有目录中解压的依赖库。
TUNDRA_TEST_SSHD=/private/test/usr/sbin/sshd \
  cargo test --locked -p platform real_isolated_ssh -- --ignored
cargo test --locked -p platform real_isolated_user_service -- --ignored
```

检查器测试须以 root 运行；SSH 测试还检查当前挂载空间与系统初始空间不同，在此私有空间提供临时 sshd 分离权限账户。若系统没有 `/run/sshd`，只在运行测试的私有挂载空间覆盖 `/run` 并创建此目录。真实服务重新加载和授权窗口仍需在对应测试主机验收。

## English summary

The existing text editor handles system files. It reads the file version and metadata, previews the content and property differences, checks a private candidate view, requests OS authorization and atomically replaces the file. Failed checks block saving; missing checkers require explicit unvalidated saving. SSH validates the main configuration, actual included fragments and host keys with `sshd -t`; systemd checks the original unit together with its drop-ins. Neither check modifies the live configuration.

Recovery records are protected by a `0700` system directory and `0600` files. Saving retains ownership, permissions, ACLs and extended attributes and rechecks the original file and parent directory after backup. Restoring a previously absent file removes the newly created file and keeps a recovery record of that removal. Reloading and checking the result are separate from saving; their failure leaves the saved changes available for further editing or manual recovery.

References: [OpenSSH sshd](https://man.openbsd.org/sshd.8), [OpenSSH Include parser](https://github.com/openssh/openssh-portable/blob/master/servconf.c), [systemd-analyze](https://www.freedesktop.org/software/systemd/man/latest/systemd-analyze.html).
