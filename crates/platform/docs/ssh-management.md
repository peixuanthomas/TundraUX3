# SSH 服务管理

SSH 页面和 `tundra-cli ssh` 读取实际的 `ssh.service`/`sshd.service`、`ssh.socket`，并通过 `sshd -T -C` 查询指定用户的有效设置。

页面显示端口、密码和公钥登录、root 登录策略、`AuthenticationMethods`、交互式认证、PAM、公钥命令和 SHA256 指纹。`Match` 条件会改变结果，查询默认可使用 localhost/127.0.0.1 条件；密码认证与交互式认证分别显示。

## 修改配置

表单支持 `Port`、`PasswordAuthentication`、`PubkeyAuthentication` 和 `PermitRootLogin`，保留 `Include`、`Match` 和其他原文。

保存流程：

1. 根据 `sshd -t -ddd` 的实际读取记录找出引用文件，移除旧的全局 `Port`，写入新的全局设置。
2. 在私有候选视图中执行 `sshd -t` 和所选用户/root 的 `sshd -T -C`。
3. 核对 `Match` 和 `ListenAddress` 不会覆盖表单选择。
4. 保存统一配置备份，启动独立恢复任务并等待就绪。
5. 原子替换配置并加载 SSH。

公钥添加和删除复用用户管理的解析、指纹和原子保存流程。

## 新连接确认

加载后在 120 秒内建立真实新连接，并在该连接中确认：

```sh
ssh -o ControlMaster=no -o ControlPath=none -p NEW_PORT user@host
tundra-cli ssh confirm TRANSACTION_ID
```

确认核对调用进程及启动时间、root 所有的 sshd 父进程、内核已建立 TCP 连接和服务器端口。sshd 会话必须在新配置加载后创建；旧会话和 ControlMaster 复用通道不能确认。新增端口必须从新增端口连接。

超时、取消、加载失败或原程序退出时，独立任务恢复文件并加载旧设置。待确认记录位于 `/var/lib/tundraux3/ssh`，开机恢复服务在 SSH 和防火墙启动前处理未确认记录，重启不会重新计时。

## 服务与防火墙

普通 sshd 通过 listener 的 SIGHUP 加载，已有会话保持运行。

`ssh.socket` 需要 `Accept=no`、单一受支持 SSH 服务和 `KillMode=process`，才可通过 socket 覆盖配置修改端口并重启 listener/socket。不符合这些关系时保持只读。

“同时放行新端口”默认关闭。开启后只向当前 UFW/firewalld 添加缺失的新 TCP 规则，并与 SSH 配置写入同一恢复记录。恢复只撤销本次新增规则，旧端口和其他规则保持不变。

## 验证

普通测试覆盖配置解析、Include、socket、输入检查、进程/TCP 状态和确认拒绝条件。

真实 OpenSSH 连接测试以 root 在私有挂载和网络空间运行：

```sh
unshare -mn -- env TUNDRA_TEST_SSHD=/private/test/usr/sbin/sshd \
  LD_LIBRARY_PATH=/private/test/usr/lib/x86_64-linux-gnu \
  /path/to/platform-test-binary \
  management::ssh::connection::real_tests::real_isolated_ssh_connections \
  --exact --ignored --nocapture --test-threads=1
```

组合防火墙与私有 systemd 测试：

```sh
bash scripts/tests/firewall-isolated.sh /path/to/platform-test-binary \
  /private/firewall-package-root ssh /private/openssh-package-root
bash scripts/tests/isolated-systemd.sh /path/to/platform-test-binary \
  0 ssh /private/openssh-package-root
```

这些用例验证新连接、旧会话、ControlMaster、错误端口、超时恢复、SIGHUP、socket 加载和防火墙回退，使用临时账户、密钥、配置和私有服务。

参考资料：[sshd](https://man.openbsd.org/sshd.8)、[sshd_config](https://man.openbsd.org/sshd_config.5)。
