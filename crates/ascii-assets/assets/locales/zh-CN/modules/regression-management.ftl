management-problem-backend-unavailable = 服务不可用。请打开详情查看连接和环境要求。
management-problem-missing-dependency = 缺少所需工具。请打开详情确认依赖。
management-problem-unsupported-environment = 当前环境不支持此功能。请打开详情查看要求。
management-problem-failure-detail = 查询或操作失败。请打开详情查看原因，再刷新重试。
management-action-query-details = 查看详情
management-systemd-not-pid1 = 当前 PID 1 是 { $process }，不是 systemd，无法管理系统服务。请使用由 systemd 启动的 Linux 环境；授权无法解决此限制。
management-service-manager-query = 无法连接 { $scope } 服务管理器：{ $error }
management-required-tool-missing = 未安装所需系统工具 { $name }。请安装对应的系统软件包后刷新。
management-tool-query-failed = { $name } 退出，状态码 { $code }。
    错误输出：{ $error }
    标准输出：{ $output }
management-requirements = 环境要求与下一步
management-firewall-unavailable = 暂无可用的防火墙规则后端，仍可查看监听端口。
management-firewalld-inactive = 未安装 UFW，firewalld 尚未运行，无法查看或修改规则。请先用系统工具配置 UFW 或启动 firewalld，或切换到监听端口。
management-firewall-tools-missing = 未安装 UFW 或 firewalld，无法查看或修改规则。请先用系统工具配置受支持的防火墙，或切换到监听端口。
management-source-unavailable = { $source }：{ $reason }
management-schedule-source = { $scope }定时器
management-schedule-limitations = 任务结果与暂停限制
management-backups-empty = 暂无恢复记录。可打开详情查看记录的产生方式和恢复范围。
management-backups-permission = 恢复记录受系统权限保护。请选择“授权查看备份”后读取。
management-backups-how-title = 如何产生记录
management-backups-how = 通过 TundraUX3 保存受支持的系统配置时，会在修改前自动保存恢复记录。请返回系统配置、网络、SSH 等管理页完成所需修改，再回到此页刷新；此页不提供手动整机备份。
management-backups-scope-title = 可恢复范围
management-backups-scope = 支持记录内的配置及账户、用户组变更，包括网络、防火墙、SSH、挂载、软件源、服务定义和定时任务设置。恢复前会检查后续修改，网络和 SSH 还需验证连接。软件包安装、普通服务启停、外部程序修改和整机快照不在恢复范围内。
