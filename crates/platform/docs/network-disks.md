# 网络与磁盘操作

## 网络连接与恢复

网络页列出接口和 NetworkManager 发现的 Wi-Fi，显示名称、信号、加密、保存和连接状态。开放网络可直接连接，加密网络在表单中输入密码，隐藏网络和其他密码入口位于“更多操作”。

连接前先在后台准备独立恢复服务，再切换网络。助手核对连接 UUID、网络名称和地址获取结果，成功后保留；密码错误、超时或取消时恢复原连接。密码通过 D-Bus 传递，不写入命令行或普通日志。

地址、网关和 DNS 修改使用 120 秒保留确认。脚本可设置 `defer_confirmation=true`，取得任务编号后查询结果并确认；超时由独立恢复服务执行。

“网络诊断”依次检查设备、无线开关、连接、地址、路由、DNS 和目标端口，结果提供再次检查、选择网络或配置入口。

网络修改、Wi-Fi 保存和忘记连接写入统一配置备份。恢复正在使用的配置前，核对文件版本、设备、MAC、原生连接和当前活动连接，然后临时应用并要求新的 120 秒确认。恢复未使用的已保存 Wi-Fi 只重新加载配置，不切换当前网络。

## 磁盘、挂载和健康

磁盘页显示字节容量和 inode 总数、剩余数与使用率。系统未报告的文件额度显示未知。

“开机挂载”生成 `/etc/fstab` 草稿并在编辑器中打开：

- 使用 UUID 标识文件系统，保留其他条目和注释。
- 已有设备路径条目可转换为 UUID。
- 重复 UUID、占用挂载点或无法确认的设备会报告具体原因。
- 保存前核对草稿所依据的原文，保存后不立即挂载。

“检查磁盘健康”优先读取 UDisks SMART，缺少有效数据时使用 `smartctl` JSON。查询不启动自检或修改磁盘设置，也不主动唤醒待机磁盘。温度、坏扇区和样本时间分别显示，缺失字段显示未知。

## 目录扫描

目录扫描使用原用户权限，跳过符号链接、其他挂载和绑定挂载，硬链接只计一次。

结果可按占用字节、内容字节或文件数量排序，排序不重新扫描。目录行进入文件管理器，文件行进入所在目录并选中文件；打开前可核对设备、文件编号和类型，返回后保留扫描结果。

## 验证与资料

后端单元测试和只读接口可在 Linux/WSL 运行。Wi-Fi 关联、密码错误恢复、断线后的独立恢复和物理盘 SMART 在对应 Linux 主机验收。

参考资料：[NetworkManager 接入点](https://www.networkmanager.dev/docs/api/latest/gdbus-org.freedesktop.NetworkManager.AccessPoint.html)、[NetworkManager 类型](https://www.networkmanager.dev/docs/api/latest/nm-dbus-types.html)、[UDisks SMART](https://storaged.org/doc/udisks2-api/latest/gdbus-org.freedesktop.UDisks2.Drive.Ata.html)、[fstab](https://man7.org/linux/man-pages/man5/fstab.5.html)。
