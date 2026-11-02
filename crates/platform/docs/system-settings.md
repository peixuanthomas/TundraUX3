# 操作系统设置

“操作系统设置 [F8]”管理主机名，“区域与时间”管理系统时区、自动校时和服务器。Linux 时钟直接跟随系统时间和时区。

## 查询与依赖

`SystemSettings` 默认查询主机名，`scope = "time"` 查询时区和校时配置。

- 主机名使用 `org.freedesktop.hostname1`，不依赖 `hostnamectl`。
- 时区使用 `org.freedesktop.timedate1`，选项由 `timedatectl list-timezones` 提供。
- timesyncd 配置使用 `systemd-analyze cat-config` 查询。
- chrony 读取实际主配置和 include 目录，`chronyc` 提供当前来源状态。

这些功能需要 systemd、系统 D-Bus 和对应服务接口。查询失败时，F4 详情显示具体原因和受限操作；某一项读取失败不会丢弃同次快照中已经取得的其他数据。

## 校时配置

支持 systemd-timesyncd 和 chrony，服务由用户现有配置决定。服务器表单可修改、清空或添加地址。

- chrony 保留 server/pool/peer 类型及其他参数，网络动态来源只读。
- timesyncd 使用独立覆盖文件，并按最终读取顺序检查合并结果。
- 保存前显示完整差异，chrony 使用 `chronyd -p` 检查候选配置。
- 全部文件核对版本后创建一份分组备份，再逐个保存；中途失败恢复先前写入的文件。
- 服务原本运行时，保存后重启并重新读取状态；原本停止时保持停止。

配置保存、服务重启和实际时间同步分别报告。恢复记录从“配置备份与恢复”查看。

## 验证

```sh
cargo test --locked -p platform --lib
scripts/tests/isolated-systemd.sh TEST_BINARY UID settings CHRONY_BUNDLE
```

真实写入测试需要 root。脚本创建独立挂载、PID、网络、主机名、D-Bus 和 systemd 环境，chrony 工具从官方包解压到私有目录，测试服务使用不执行校时的占位进程。

参考资料：[systemd 主机名接口](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.hostname1.xml)、[系统时间接口](https://github.com/systemd/systemd/blob/main/man/org.freedesktop.timedate1.xml)、[chronyd](https://chrony-project.org/doc/4.7/chronyd.html)。
