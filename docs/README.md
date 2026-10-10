# 文档索引

[English](../README.md) · [简体中文](README.zh-CN.md)

## 从这里开始

| 需要了解什么 | 入口 |
| --- | --- |
| 安装、运行与当前限制 | [项目 README](README.zh-CN.md) |
| 整体分工、构建与打包 | [技术文档](README-TECHNICAL.md) |
| 检查命令、PTY 和 CI | [测试指南](testing.md) |
| 所有页面的共同交互要求 | [UI 要求](UI-requirements.md) |
| 已发布版本的变化 | [CHANGELOG](CHANGELOG.md) |
| Linux 运行依赖和便携安装 | [Linux 说明](packaging/linux/README-LINUX.txt) |

## 按功能查找

| 功能 | 文档 |
| --- | --- |
| 页面、焦点、返回与弹窗 | [Shell](../crates/shell/README.md) |
| 文件、编辑器、启动器与应用状态 | [app](../crates/app/README.md) |
| 系统服务、进程、软件包、网络、磁盘和账户 | [platform](../crates/platform/README.md) |
| 命令行与脚本 | [CLI](../crates/cli/README.md) |
| 批准、密码和管理任务 | [AutoAdmin](../crates/auto-admin/README.md) |
| 日志、查询、跟随、导出和清理 | [runtime-log](../crates/runtime-log/README.md) |
| 语言资源与恢复 | [i18n](../crates/i18n/docs/localization.md) |
| 保存、格式校验和损坏恢复 | [storage](../crates/storage/README.md) |
| 异常报告和后台任务 | [watchdog](../crates/watchdog/README.md) |

其他 crate 见[完整分工表](README-TECHNICAL.md#crate-分工)。单个 crate 的说明放在 `crates/<crate>/README.md` 或其 `docs/`；本目录只保留跨 crate 的说明、规范、发行和历史验证记录。

`packaging/releases/` 记录对应已发布版本；`packaging/linux/VALIDATION-*.md` 是当时的验证记录，不能据此推断当前仍支持旧系统包更新。
