# SSH 服务管理

SSH 页面和 `tundra-cli ssh` 读取 systemd 中实际的 `ssh.service` / `sshd.service`、`ssh.socket`，并用 `sshd -T -C` 查询所选用户的有效设置。页面展示端口、密码和公钥登录、root 登录策略、`AuthenticationMethods`、交互式认证、PAM、公钥命令及公钥 SHA256 指纹。读取失败会显示错误，不把未知状态当作关闭。

页面、错误和确认提示使用当前语言。动作编号、配置指令和事务编号保持固定；CLI 从单独的事务事件取得编号，翻译提示不会影响确认或状态查询。

`Match` 可以按用户、来源和目标地址改变设置。查询结果会列出这次检查使用的连接条件；默认条件是 localhost / 127.0.0.1。密码认证关闭时，交互式认证仍可能接受密码；两项分别显示。

## 修改与新连接确认

表单修改 `Port`、`PasswordAuthentication`、`PubkeyAuthentication`、`PermitRootLogin`。`Include` 和 `Match` 原文保留。程序从 `sshd -t -ddd` 的实际读取记录找出引用文件，删除其中旧的全局 `Port`，然后在主配置前写入选择的全局设置。检查在单独进程的私有挂载目录进行，正式文件此时保持原样；候选同时通过 `sshd -t` 和所选用户 / root 的 `sshd -T -C` 才能继续。若 `Match` 或带固定端口的 `ListenAddress` 覆盖表单选择，程序拒绝保存，要求先在配置编辑器检查对应规则。

修改使用统一的配置备份记录，并保留文件的所有者、权限、ACL 和扩展属性。程序先写入 root 私有的待确认记录，启动独立的 root 恢复任务并等它报告就绪，最后才写正式配置和加载 SSH。公钥添加与删除复用用户管理的解析、指纹和原子替换代码，同样经过本流程。

加载完成后有 120 秒确认时间。保持原程序打开，在另一条真正的新 SSH 连接中执行：

```sh
ssh -o ControlMaster=no -o ControlPath=none -p NEW_PORT user@host
# 在新连接中：
tundra-cli ssh confirm TRANSACTION_ID
```

确认会核对授权连接提供的调用进程编号、进程创建时间、root 所有的 sshd 父进程、内核中的已建立 TCP 连接以及服务器端口。sshd 会话进程必须在新配置加载完成后创建。新增监听端口时，必须从新增端口连接确认；只修改认证设置或删除端口时，从保留的监听端口建立新连接确认。旧会话、客户端连接共享产生的新通道、手工设置 `SSH_CONNECTION` 都不能确认。确认命令的连接或配置检查失败时，立即恢复旧设置。

超时、取消、加载失败或原程序退出后，独立任务恢复文件并重新加载旧设置。待确认记录和恢复助手位于 `/var/lib/tundraux3/ssh`，只有 root 可访问。开机恢复服务在 SSH 服务、socket 和防火墙启动前处理尚未确认的记录；记录包含启动编号，重启不会重新开始计时。尚未运行的防火墙通过官方离线命令处理本次保存的规则，随后服务按恢复后的设置启动。恢复前比较当前文件和本次候选，发现外部改动会保留备份并报告恢复失败，避免覆盖其他人的修改。

## 服务加载与防火墙

普通 sshd 使用 listener 的 `SIGHUP`，等待它重新创建监听 socket 后才接受新连接确认。不会停止已有会话。

`ssh.socket` 必须为 `Accept=no`，只触发一个受支持的 SSH 服务，该服务必须使用 `KillMode=process`。只有满足这些条件，才通过 socket 的覆盖配置修改端口，并停止 / 启动 listener 与 socket。`KillMode=process` 让旧连接所属的子进程继续运行。自定义 `-f` / `-p` / `-o` 服务启动参数、无法确认的进程或不同的 socket 关系会阻止修改。

“同时放行新端口”默认关闭。开启时，只在当前运行的 UFW / firewalld 添加缺失的新 TCP 规则，旧端口和原有规则保留。新增句柄在应用前写入恢复记录；恢复只撤销本次记录的新增规则。SSH 和附带规则共用一条完整操作记录；UFW 原文件也保存在其中，但规则恢复通过 UFW 的原生命令执行，不直接覆盖整个规则文件。从历史主动恢复会一并撤销原操作新增规则；取消这次恢复时，再补回它刚撤销的原规则，其他规则保留。firewalld 无法给规则附加任意事务标识，程序使用事务对应的优先级及精确规则比较；外部工具同时创建完全相同规则时，系统无法区分它们的来源。

## 验证范围

普通测试覆盖配置内容保留、实际 Include 读取记录、socket 地址保留、输入检查、内核进程 / TCP 信息解析，以及旧连接、错误端口和连接共享的拒绝条件。Linux 测试使用 WSL。

`real_isolated_ssh_connections` 是另行启用的真实 OpenSSH 测试，必须以 root 在私有挂载和网络空间运行，并提供只解压、不安装的 OpenSSH server 与运行库。测试用临时账户文件、密钥和配置分别运行普通 sshd listener 与 `systemd-socket-activate` listener，检查 root 和普通用户的新连接通过、旧会话及 ControlMaster 通道拒绝、错误端口拒绝，以及更换 listener 后旧会话继续工作。它还用独立 root 子进程调用正式恢复函数，检查超时、发起程序退出、取消、无效确认、启动编号变化后的文件恢复和旧端口重新监听。

```sh
# 先用正常账户编译测试程序，再在私有空间运行对应测试文件。
unshare -mn -- env TUNDRA_TEST_SSHD=/private/test/usr/sbin/sshd \
  LD_LIBRARY_PATH=/private/test/usr/lib/x86_64-linux-gnu \
  /path/to/platform-test-binary \
  management::ssh::connection::real_tests::real_isolated_ssh_connections \
  --exact --ignored --nocapture --test-threads=1
```

同时验证真实 UFW 时，使用共享防火墙隔离脚本。它先检查测试名称确实存在，再在私有挂载、网络及进程空间运行，并比较测试前后的宿主 nftables 规则。组合用例保留旧的 5555/TCP 规则，每次只撤销本次新增端口；启动编号变化的用例还检查防火墙尚未启用时的保存规则恢复。故意让 UFW 命令失败时，仍检查 SSH 已恢复旧端口，恢复 UFW 后再重试回退。

```sh
bash scripts/tests/firewall-isolated.sh /path/to/platform-test-binary \
  /private/firewall-package-root ssh /private/openssh-package-root
```

恢复测试的 systemctl 返回值来自私有挂载目录中的固定测试程序，真实 sshd 进程和文件恢复均实际运行。实际 systemd unit 的 Restart / KillMode 行为、操作系统授权窗口以及真正整机重启恢复，仍需要可丢弃的 Linux 测试主机验收。测试不会修改开发主机的真实 SSH 或防火墙。

参考：[OpenSSH sshd](https://man.openbsd.org/sshd.8)、[sshd_config](https://man.openbsd.org/sshd_config.5)。
