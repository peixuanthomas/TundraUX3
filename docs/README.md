# 文档索引

[English](../README.md) · [简体中文](README.zh-CN.md)

## 入门与开发

| 内容 | 文档 |
| --- | --- |
| 功能、安装与界面操作 | [中文 README](README.zh-CN.md) · [English README](../README.md) |
| 架构、构建、测试与打包概览 | [技术文档](README-TECHNICAL.md) |
| 自动测试、真实终端与平台验收 | [测试指南](testing.md) |
| 界面交互与验收标准 | [UI 要求](UI-requirements.md) |
| Linux 运行依赖与便携更新 | [Linux 运行说明](packaging/linux/README-LINUX.txt) |
| 随包 Vim 的构建与发布 | [随包 Vim](packaging/vim.md) |

## 功能说明

| 内容 | 文档 |
| --- | --- |
| 普通编辑器、Vim 与脚本调试 | [编辑器与脚本工作区](source-editor.md) |
| 编辑器大文件计算与基准 | [编辑器大文件支持](editor-scalability.md) |
| 各页面的列表多选行为 | [列表多选](list-selection.md) |
| Linux 服务、软件包、网络与磁盘 | [Linux 系统管理](../crates/platform/docs/linux-management.md) |
| CLI 命令、诊断与 UI 控制 | [CLI 说明](../crates/cli/README.md) |
| 内置应用与启动器 | [应用说明](../crates/app/docs/applications.md) |
| 日志查询与导出 | [日志说明](../crates/runtime-log/docs/logs.md) |
| 语言资源与恢复 | [本地化说明](../crates/i18n/docs/localization.md) |

## Crate 文档

| Crate | 职责与文档入口 |
| --- | --- |
| [shell](../crates/shell/README.md) | 页面、导航、输入、弹窗与整帧合成 |
| [app](../crates/app/README.md) | 应用状态、文件、编辑器、启动器与更新 |
| [ui](../crates/ui/README.md) | 共享控件、布局、主题、绘制与动画 |
| [cli](../crates/cli/README.md) | 外部命令、REPL、诊断与 UI 控制 |
| [platform](../crates/platform/README.md) | 文件、进程、软件包和操作系统接口 |
| [terminal-runtime](../crates/terminal-runtime/README.md) | 宿主终端与子终端 |
| [vim-runtime](../crates/vim-runtime/README.md) | Vim 进程、缓冲同步与保存 |
| [auto-admin](../crates/auto-admin/README.md) | 授权、密码输入与管理任务 |
| [storage](../crates/storage/README.md) | 配置格式、原子保存、迁移与恢复 |
| [identity](../crates/identity/README.md) | 系统用户与本地应用账户 |
| [system-services](../crates/system-services/README.md) | 系统与天气快照、后台服务 |
| [runtime-log](../crates/runtime-log/README.md) | 日志记录、查询与事故报告 |
| [watchdog](../crates/watchdog/README.md) | 进程和任务监督、恢复记录 |

其余 crate 见[完整分工表](README-TECHNICAL.md#crate-分工)。实现细节放在 `crates/<crate>/README.md` 或其 `docs/` 中。

## 历史资料

以下文件按当时状态保留，不作为现行使用指南：

- [CHANGELOG](CHANGELOG.md) 与[版本发布说明](packaging/releases/v1.5.md)
- [Linux 管理验收记录](linux-administration-validation.md)
- [操作可用性验收记录](operations-usability-validation.md)
- [界面验收记录](ui-review-2026-10-31.md)
- [Linux 便携包历史验证](packaging/linux/VALIDATION-2026-09-18.md)
