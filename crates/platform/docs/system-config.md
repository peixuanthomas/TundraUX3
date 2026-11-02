# 系统配置文件

服务覆盖配置、软件源、fstab 和其他系统文件都在同一个文本编辑器中修改。保存流程包括版本核对、差异预览、候选检查、系统授权、备份和原子替换。

## 读取与检查

读取时记录文件内容版本、所有者、用户组和权限。保存前显示内容与属性差异，并在私有候选视图中运行对应检查器。

- SSH 使用 `sshd -t -ddd` 检查主配置、实际读取的 Include 片段和主机密钥。
- systemd 使用 `systemd-analyze verify`，保留原单元和其他覆盖配置；用户服务使用对应 `SYSTEMD_UNIT_PATH`。
- fstab 使用 `findmnt --verify`，软件源使用发行版对应的解析检查。
- 校时配置可一次检查多个候选文件，并按一次操作分组备份。

候选内容只出现在私有连接和临时检查目录。检查失败会阻止保存；没有可用检查器时，需要明确选择未检查保存。

## 保存与恢复

写入前再次核对原文件和父目录，保存通过原子替换完成，并保留所有者、用户组、权限、ACL 和扩展属性。新文件继承最近的已有父目录所有者，路径逐层拒绝符号链接。

恢复记录位于 `/var/lib/tundraux3/config-backups`，目录权限为 `0700`，文件权限为 `0600`。记录包含原内容、属性和修改后版本。

恢复前重新预览差异、核对当前版本并检查候选配置。恢复“原先不存在”会删除本次创建的文件，同时保留这次删除的恢复记录。

多文件保存在中途失败时，按本次保存后的版本恢复先前写入的文件；发现外部改动则停止覆盖并报告冲突。

## 保存后的加载

默认只保存配置。

- SSH 重新加载需要此次保存的备份编号，并在 120 秒内通过真实新连接执行 `tundra-cli ssh confirm ID`，否则恢复旧配置。
- systemd 默认重新读取服务定义；`reload_service=true` 时同时重载服务。
- 配置保存、服务加载和运行状态分别报告，加载失败后可查看日志或从备份恢复。

CLI 使用：

```sh
tundra-cli system-config read|diff|check|apply|permissions|history|restore|reload …
```

脚本保存必须提供 `--expected-version`，未检查保存必须显式提供 `--allow-unvalidated`。内建 Command Line 使用 `/system-config …`。

## 验证

```sh
TUNDRA_TEST_SSHD=/private/test/usr/sbin/sshd \
  cargo test --locked -p platform real_isolated_ssh -- --ignored
cargo test --locked -p platform real_isolated_user_service -- --ignored
```

普通测试覆盖版本冲突、外部改动、属性保留、备份权限和恢复。真实检查器测试以 root 在私有挂载空间运行，使用临时 `/etc` 和检查工具，不修改宿主配置。

参考资料：[OpenSSH sshd](https://man.openbsd.org/sshd.8)、[OpenSSH Include parser](https://github.com/openssh/openssh-portable/blob/master/servconf.c)、[systemd-analyze](https://www.freedesktop.org/software/systemd/man/latest/systemd-analyze.html)。
