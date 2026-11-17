# Desk Pet 文档

本目录是 Desk Pet 的完整工程文档。README 负责「项目是什么、怎么跑起来」，这里回答「怎么用、怎么设计、
怎么改、怎么发版」。

## 文档索引

| 文档 | 适合谁 | 内容 |
| --- | --- | --- |
| [../README.md](../README.md) | 所有人 | 项目简介、功能总览、快速开始、平台支持、已知限制 |
| [architecture.md](architecture.md) | 开发者 / 维护者 | 多窗口模型、启动时序、前端后端分层、事件与数据流、关键设计决策 |
| [features.md](features.md) | 使用者 / 产品 | 每个功能怎么用、行为边界、窗口入口、数据同步规则 |
| [development.md](development.md) | 前端 / Rust 开发者 | 环境、命令、代码约定、UnoCSS/i18n、扩展新页面 / 工具 / 命令、调试排错 |
| [backend.md](backend.md) | Rust 开发者 | workspace crate 职责、全部 Tauri 命令、事件、SQLite 表、后台任务、外部协议 |
| [release.md](release.md) | 发布负责人 | Makefile 发布、CI job、签名密钥、更新器、hub 上传、回滚 |
| [clipboard-sync-api.md](clipboard-sync-api.md) | 服务端 / 同步开发者 | 剪贴板云同步接口契约、表结构、去重与软删除逻辑 |
| [index.html](index.html) | 外部访问者 | 产品展示页（静态站点） |

## 推荐阅读路径

### 第一次参与开发

1. [../README.md](../README.md) 的「快速开始」跑通 `npm run tauri dev`
2. [architecture.md](architecture.md) 理解主窗口与独立窗口、前后端边界
3. [development.md](development.md) 的「代码约定」与「扩展指南」
4. [backend.md](backend.md) 查命令、事件、表结构

### 改一个功能

1. 在 [features.md](features.md) 找到对应模块，确认行为边界
2. 前端按 `views` / `components` / `stores` / `api` 分层定位
3. Rust 侧按 `src-tauri/src/tauri_cmds/<域>.rs` 定位命令
4. 数据层改动先看 [backend.md](backend.md) 的「SQLite 表」与迁移约定
5. 完成后按 [development.md](development.md) 的验证清单跑类型检查与测试

### 只关心发布

直接看 [release.md](release.md)。

## 文档维护约定

- 涉及命令、事件、表结构、窗口 label 的改动，同步更新 [backend.md](backend.md) 或
  [architecture.md](architecture.md)。
- 涉及用户可见行为、限制、平台差异的改动，同步更新 [features.md](features.md) 与 README 的「已知限制」。
- 新增 crate、目录、窗口或工具时，同步更新 README 的「工程结构」。
- 文案以中文为主；命令、路径、类型名、事件名保持英文原文，且与代码完全一致，便于全局搜索。
- 文档中的命令都应可直接复制执行；不确定的内容宁可标注「当前实现」而不是写死推测。
