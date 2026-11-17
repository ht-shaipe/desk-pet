# Desk Pet 桌面精灵

基于 **Tauri 2 + Vue 3 + TypeScript + Element Plus + UnoCSS** 的跨平台桌面宠物。
精灵常驻桌面，单击展开面板；面板内集成启动器、剪贴板、收藏、工具箱、番茄钟、AI 助手、账户与设置，
并把图库、文档、接口请求、文本对比、磁盘分析、系统监控等重功能放到独立窗口。

> 完整文档索引见 **[docs/README.md](docs/README.md)**。
> 使用说明、架构设计、后端命令、开发约定与发布流程分别有独立文档。

---

## 目录

- [功能总览](#功能总览)
- [技术栈](#技术栈)
- [架构速览](#架构速览)
- [工程结构](#工程结构)
- [快速开始](#快速开始)
- [文档导航](#文档导航)
- [数据与本地文件](#数据与本地文件)
- [平台支持](#平台支持)
- [发布与自动更新](#发布与自动更新)
- [已知限制](#已知限制)
- [第三方许可](#第三方许可)

---

## 功能总览

### 桌面精灵与主面板

- 无边框、透明、始终置顶、不占任务栏，macOS 下可见于所有工作区
- 按住精灵自由拖拽，位置自动记忆并在重启后恢复；拖出屏幕会被钳制回可见区域
- 单击精灵展开面板，再次单击精灵或点右上角按钮收起
- 折叠态与展开态是**同一个窗口的程序化 resize**（默认 72×72 ⇄ 388×588），无需多窗口同步
- 收起时可选在精灵下方显示**当前时间**或**今日 AI token 用量**徽标（番茄钟运行时优先显示倒计时）
- 支持自定义头像、图标大小、主题（浅色 / 深色 / 跟随系统）、主题色、应用显示名称
- 系统托盘：显示 / 隐藏精灵、悬浮监控、退出
- 全局快捷键：`Command/Ctrl + Shift + P` 显示 / 隐藏
- 单实例运行：重复启动会唤起已有实例；支持开机自启

### 主面板标签页

| 标签 | 视图 | 主要能力 |
| --- | --- | --- |
| AI 助手 | `chat` | OpenAI 兼容流式对话、函数调用、内置工具、MCP 外部工具、聊天记录持久化 |
| 剪贴板 | `clipboard` | 文本 / 图片历史、搜索与类型筛选、重复次数、回贴、单条删除、按类型 / 时间批量清理、云同步 |
| 收藏 | `stash` | 仓库 / 文章 / 视频 / 商品 / 社交 / 图片 / 书签 / 便签八类收藏、分类标签、导入导出、GitHub/Gitee 数据刷新、云同步 |
| 工具箱 | `toolbox` | 工具注册表 + 详情页，工具组件按需懒加载 |
| 快速启动 | `launcher` | 应用 / 网址 / 命令三类入口，分类展示，增删改、自定义图标与颜色、应用图标提取 |
| 番茄钟 | `pomodoro` | 专注 / 短休 / 长休循环、自定义时长、完成次数、系统通知 + 提醒浮层 |
| 账户 | `account` | 手机号注册 / 登录、图形验证码或滑块验证、token 本地保存、云同步开关 |
| 设置 | `settings` | 通用、外观、关于三组设置与更新检查 |

### 内置工具箱

工具箱采用「注册表 + 懒加载」架构（`src/components/toolbox/registry.ts`），当前内置：

| id | 工具 | 说明 |
| --- | --- | --- |
| `qr` | 二维码 | 生成、解析；支持粘贴 / 选择图片、复制与保存 PNG |
| `base64` | Base64 | 文本与 Base64 互转 |
| `des` | DES 加解密 | DES ECB 文本加解密（Base64 密文） |
| `imgcompress` | 图片压缩 | 原生压缩引擎、预设 / 目标体积 / 格式选择、文字 / 图片 / 盲水印、目录批量压缩 |
| `http` | 接口请求 | 独立窗口，Rust 代理发送，查看请求 / 响应头、耗时、体积 |
| `cleanup` | 磁盘清理 | 规则化候选扫描、复核清单、回收站 / 永久删除、本地 SSE 进度 |
| `docconv` | 文档转换 | 任意格式文档转 Markdown（PDF / DOCX / XLSX / PPTX / EPUB / ODT / CSV 等） |
| `monitor` | 系统监控 | CPU / 内存 / 网络速率 / 磁盘卷；悬浮监控条；macOS 菜单栏文字 |
| `textdiff` | 文本对比 | 独立窗口，行级 + 词级 diff，tree-sitter 语法高亮 |
| `formatter` | 代码格式化 | Prettier（JSON / JS / TS / HTML / Vue / CSS / SCSS / LESS / YAML） |
| `aiusage` | AI 用量 | Claude Code / Codex CLI / OpenCode 本地日志增量统计，按今日 / 昨日 / 本周 / 本月聚合 |

### 独立窗口与系统级功能

| 窗口 label | 入口 | 说明 |
| --- | --- | --- |
| `main` | 应用启动 | 精灵 + 面板，二者共用同一窗口 |
| `notify` | 番茄钟完成 | `notify.html` 独立入口的透明提醒浮层 |
| `analyzer` | 工具箱 → 磁盘清理 → 分析 | 磁盘全树扫描、treemap、标记与删除复核 |
| `images` | 工具箱 → 图片 → 图库 | 图库目录增量扫描、EXIF、缩略图、标签、收藏、精确去重 |
| `docs` | 工具箱 → 文档转换 → 管理 | 文档目录增量扫描、检索、转 Markdown、移废纸篓 |
| `http` | 工具箱 → 接口请求 | HTTP 请求调试窗口 |
| `textdiff` | 工具箱 → 文本对比 | 文本差异对比窗口 |
| `monitorbar` | 托盘 / 监控工具 | 无边框悬浮监控条，支持鼠标穿透 |
| `updater` | 检测到新版本 | 折叠态下的更新提示小窗 |

其他系统能力：

- 快捷操作：深浅模式切换、静音、锁屏、睡眠、跳转系统设置子页（深浅模式 / 静音仅 macOS）
- 剪贴板后台轮询（文本 + 图片），去重、重复计数、跨重启签名
- AI 编程用量后台每 5 分钟增量扫描本地日志
- MCP 服务器随应用启动自动连接（stdio 子进程 / Streamable HTTP）
- 基于 gRPC 双向流的实时通道：设备注册、心跳、任务下发、推送消息、登录态绑定

### 账户与云同步

- 复用 `hub.htui.cc` 账户体系：手机号 + 密码，注册 / 登录支持图形验证码或滑块验证
- token 保存在 `settings.json`，所有云同步接口使用裸 `Authorization: <token>`（不带 `Bearer`）
- 收藏云同步：批量 upsert + 游标分页全量拉取 + 云端删除墓碑，LWW（last-write-wins）合并
- 剪贴板云同步：批量 upsert + 游标分页增量拉取；**只同步文本，图片仅保留本地**
- 启动、打开对应页面、手动操作与 5 分钟节流共同触发同步

### AI 助手

- 任意 OpenAI 兼容 `/chat/completions` 服务：自定义 `base_url`、`api_key`、模型名
- 流式输出、Markdown 渲染、工具调用 agent 循环（最多 8 轮）
- 内置工具：`list_apps`、`open_app`、`open_pomodoro`、`open_view`、`open_tool`
- 外部工具：MCP 服务器（stdio / HTTP），调用前弹确认框
- 聊天记录落 SQLite，支持删除单条、从某条起截断重试、清空
- 本机 / 内网 `https` 自签名证书自动回退（仅限私有地址）

---

## 技术栈

| 层 | 技术 |
| --- | --- |
| 桌面壳 | Tauri 2（透明窗口、托盘、全局快捷键、单实例、自动更新、通知、自启动） |
| 前端 | Vue 3 `<script setup>` + TypeScript、Pinia、vue-i18n、Element Plus 2.13、UnoCSS |
| 构建 | Vite 8（主入口 + `notify.html` 多页面）、vue-tsc |
| 数据 | SQLite（rusqlite bundled，WAL）+ `settings.json` |
| 系统能力 | Rust 2024、arboard、image / webp / png、sysinfo、objc2（macOS）、tree-sitter、anydoc |
| 网络 | tauri-plugin-http、reqwest、tonic（gRPC）、axum（本地 SSE） |
| 扩展协议 | MCP（rmcp 0.8，stdio / Streamable HTTP） |

---

## 架构速览

```text
┌──────────────────────────── WebView (Vue 3) ────────────────────────────┐
│  src/views/*.vue        面板页面（8 个标签）                              │
│  src/*App.vue           独立窗口根组件（analyzer/images/docs/http/...）  │
│  src/stores/*           Pinia 状态（配置 / 剪贴板 / 收藏 / 番茄钟 / UI） │
│  src/composables/*      云同步、通道、拖拽、更新器等组合逻辑              │
│  src/api/*              Tauri invoke 的按域封装                           │
└───────────────────────────────────┬─────────────────────────────────────┘
                                    │ invoke / event / img:// 协议
┌───────────────────────────────────▼─────────────────────────────────────┐
│ src-tauri（Tauri 壳）                                                    │
│  ├─ tauri_cmds/*     命令薄壳：参数校验、任务编排、事件转发              │
│  ├─ channel.rs       gRPC 实时通道（心跳 / 重连 / 任务 / 推送）          │
│  ├─ mcp.rs           MCP 客户端管理                                      │
│  ├─ ai_usage.rs      AI 日志增量解析                                     │
│  ├─ tray.rs          托盘与窗口显隐                                      │
│  └─ state.rs         AppState（Store / 剪贴板缓存 / 通道 / MCP / 任务）  │
└───────┬───────────────────┬───────────────────┬─────────────────────────┘
        │                   │                   │
┌───────▼────────┐ ┌────────▼────────┐ ┌────────▼─────────┐
│ deskpet-store  │ │ deskpet-os      │ │ deskpet-disk     │
│ SQLite / 设置  │ │ 系统能力 / 文件 │ │ 扫描 / 删除护栏  │
│ 迁移 / 头像    │ │ 图片 / 监控     │ │ 分类 / treemap   │
└────────────────┘ └─────────────────┘ └──────────────────┘
                                    ┌────────────────────┐
                                    │ channel-client     │
                                    │ gRPC 双向流客户端  │
                                    └────────────────────┘
```

关键设计：

- **窗口**：主窗口是「精灵 + 面板」二合一，通过 `windowCtl.ts` 程序化改变尺寸与位置；其余窗口由 Rust 侧
  `WebviewWindowBuilder` 按需创建，通过 `index.html?view=...` 挂载不同根组件。
- **分层**：前端只依赖 `src/api`；Rust 命令只做薄壳，业务逻辑落在 `deskpet-store` / `deskpet-os` /
  `deskpet-disk`，可脱离 Tauri 单独测试。
- **事件**：剪贴板、扫描进度、通道消息、更新状态等通过 Tauri 事件从 Rust 推送到前端；磁盘扫描进度走
  本地 `127.0.0.1` 随机端口的 SSE。
- **数据**：SQLite 表结构在 `crates/deskpet-store/src/store.rs` 的 `DDL` 中集中定义；启动时幂等补列并迁移旧 JSON。

更完整的窗口模型、启动时序、事件与数据流见 [docs/architecture.md](docs/architecture.md)。

---

## 工程结构

```text
desk-pet/
├── Cargo.toml                  # Rust workspace 根（统一依赖版本与 release 配置）
├── package.json                # 前端依赖与脚本
├── uno.config.ts               # UnoCSS 主题 token、快捷类、动画
├── vite.config.ts              # 端口 1820 / HMR 1821；main + notify 多页面
├── index.html                  # 主入口（主窗口与独立窗口共用）
├── notify.html                 # 番茄钟提醒浮层入口
├── docs/
│   ├── README.md               # 文档索引
│   ├── architecture.md         # 架构与运行时设计
│   ├── features.md             # 功能使用手册
│   ├── development.md          # 开发约定与扩展指南
│   ├── backend.md              # Rust 命令、事件、数据模型
│   ├── release.md              # 版本发布与自动更新
│   ├── clipboard-sync-api.md   # 剪切板云同步接口契约
│   ├── index.html              # 产品展示页
│   └── assets/                 # 展示页静态资源
├── src/
│   ├── main.ts                 # 按 ?view= 选择根组件并挂载
│   ├── notify-main.ts          # 提醒浮层入口（禁止导入 virtual:uno.css）
│   ├── App.vue                 # 主窗口：精灵 + 面板
│   ├── AnalyzerApp.vue         # 磁盘分析窗口
│   ├── ImagesApp.vue           # 图库窗口
│   ├── DocsApp.vue             # 文档管理窗口
│   ├── HttpApp.vue             # 接口请求窗口
│   ├── TextDiffApp.vue         # 文本对比窗口
│   ├── FloatingMonitorApp.vue  # 悬浮监控条
│   ├── UpdaterWindow.vue       # 更新提示小窗
│   ├── views/                  # 面板页面
│   ├── components/
│   │   ├── layout/             # 面板容器与导航
│   │   ├── pet/                # 精灵
│   │   ├── disk/               # treemap 等
│   │   └── toolbox/            # 工具注册表 + tools/
│   ├── stores/                 # Pinia
│   ├── composables/            # 云同步 / 通道 / 拖拽 / 更新
│   ├── api/                    # invoke 封装
│   ├── lib/                    # windowCtl / theme / stash / ai-tools 等
│   ├── i18n/                   # zh-CN / en
│   ├── types/                  # 前后端共享类型
│   └── styles/main.css         # 少量全局样式
├── src-tauri/
│   ├── src/lib.rs              # Tauri Builder、插件、命令注册、启动时序
│   ├── src/tauri_cmds/         # 按域拆分的命令
│   ├── src/channel.rs          # gRPC 通道运行时
│   ├── src/mcp.rs              # MCP 客户端
│   ├── src/ai_usage.rs         # AI 用量扫描
│   ├── src/tray.rs             # 托盘
│   ├── src/state.rs            # AppState
│   ├── capabilities/           # Tauri 权限声明
│   └── tauri.conf.json         # 窗口、打包、更新器配置
└── crates/
    ├── deskpet-store/          # SQLite 持久化、设置、迁移、头像、图库/文档/用量数据层
    ├── deskpet-os/             # 应用启动、快捷操作、剪贴板、文件、图片、监控、菜单栏、窗口圆角
    ├── deskpet-disk/           # 磁盘扫描、计量、分类、删除护栏、treemap、insights
    └── channel-client/         # gRPC 双向流客户端（proto 副本 + 已生成代码）
```

---

## 快速开始

前置要求：

- Node.js 20+
- Rust stable 工具链（`rustup`）
- macOS 需要 Xcode Command Line Tools；Windows 需要 WebView2 与 MSVC 构建工具
- 开发 macOS 透明窗口需要系统允许终端 / IDE 的屏幕相关权限（首次运行由系统弹窗确认）

```bash
npm install

npm run tauri dev     # 开发调试（Vite 1820 + Tauri 窗口）
npm run dev           # 只启动 Vite（浏览器调试用，Tauri API 不可用）
npm run build         # 前端构建 + vue-tsc 类型检查
npm run tauri build   # 打包当前平台安装包
```

常用校验：

```bash
npm run build               # 前端类型检查 + 构建
cargo check                 # Rust 静态检查
cargo check --workspace     # 含所有 crate
cargo test --workspace      # Rust 全部测试
make check                  # 前端构建 + cargo check --workspace
```

> `npm run tauri build` 需要更新器签名私钥才会产出可发布的更新包；本地调试可只跑 `npm run build`。

---

## 文档导航

| 文档 | 内容 |
| --- | --- |
| [docs/README.md](docs/README.md) | 文档索引与推荐阅读路径 |
| [docs/architecture.md](docs/architecture.md) | 多窗口模型、启动流程、前后端分层、事件与数据流、关键设计决策 |
| [docs/features.md](docs/features.md) | 面向使用者的完整功能手册与行为边界 |
| [docs/development.md](docs/development.md) | 环境、命令、代码约定、UnoCSS/i18n、扩展新页面 / 工具 / 命令、调试与排错 |
| [docs/backend.md](docs/backend.md) | Rust workspace、全部 Tauri 命令、事件、SQLite 表、后台任务、外部协议 |
| [docs/release.md](docs/release.md) | Makefile 发布流程、GitHub Actions、签名、更新器、hub 上传 |
| [docs/clipboard-sync-api.md](docs/clipboard-sync-api.md) | 剪贴板云同步服务端接口契约 |

---

## 数据与本地文件

应用数据目录（Tauri `app_data_dir`）：

- macOS：`~/Library/Application Support/com.deskpet.desktop/`
- Windows：`%APPDATA%\com.deskpet.desktop\`

| 路径 | 说明 |
| --- | --- |
| `deskpet.db`（含 `-wal` / `-shm`） | 主数据库：剪贴板、收藏、启动器、聊天、图库、文档、AI 用量 |
| `settings.json` | 应用设置、模型配置、MCP 配置、云同步开关与游标、token |
| `avatar.<ext>` | 自定义精灵头像 |
| `clip_images/` | 剪贴板图片文件 |
| `image_thumbs/` | 图库缩略图（两级分桶） |
| `icons/` | 启动器应用图标缓存 |
| `debug.log` | 前端调试日志（超过 512 KB 保留尾部一半） |
| `*.json.bak` | 旧版 JSON 数据迁移后的备份 |

首次运行会自动把旧版 `notes.json` / `clipboard.json` / `launcher.json` 导入 SQLite，并把原文件重命名为
`.json.bak`；旧便签会并入收藏（`stash://note/<id>`）。所有迁移幂等，可重复启动。

网络端点：

| 用途 | 地址 |
| --- | --- |
| 账户 / 收藏 / 剪贴板 / 发布 | `https://hub.htui.cc` |
| 更新清单 | `https://hub.htui.cc/apps/desk-pet/latest.json`、GitHub Releases |
| gRPC 实时通道 | `https://hub.htui.cc`（`channel.v1.ChannelService/Open`） |
| AI 模型 | 用户自定义 OpenAI 兼容地址 |

---

## 平台支持

| 功能 | macOS | Windows |
| --- | --- | --- |
| 漂浮 / 置顶 / 拖拽 / 位置记忆 | 支持 | 支持 |
| 面板全部标签页 | 支持 | 支持 |
| 图库 / 文档 / 磁盘分析 / 接口请求 / 文本对比 | 支持 | 支持 |
| 深浅模式切换、静音 | 支持（首次需授权自动化） | 暂不支持 |
| 锁屏 / 睡眠 / 系统设置跳转 | 支持 | 支持 |
| Dock 显示开关 | 支持 | — |
| 悬浮监控条 | 支持 | 支持 |
| 菜单栏监控文字 | 支持 | — |
| 磁盘「完全磁盘访问」引导 | 支持 | — |

> Linux（尤其 Wayland 下的透明、置顶与拖拽窗口）暂不在支持范围内。

---

## 发布与自动更新

发布由 GitHub Actions（`.github/workflows/release.yml`）驱动：推送 `v*` tag 或手动触发，构建
macOS（arm64 / x64）与 Windows 安装包，上传 GitHub Release，合并生成 `latest.json`，并可推送到 hub 发版接口。

应用内置 `tauri-plugin-updater`：启动静默检查、每 6 小时周期检查；折叠态发现新版本时弹独立更新小窗，
展开态则跳转「设置 → 关于」。

常用发布命令：

```bash
make release                    # 自动 bump patch 并发布
make release '修复xxx问题'       # 提交说明 + 发布
make release VERSION=0.2.0      # 指定版本
make minor / make major
make dry-run                     # 预览发布步骤
```

签名密钥、Secrets、CI job 与 hub 上传契约详见 [docs/release.md](docs/release.md)。

---

## 已知限制

- 精灵窗口为矩形透明窗，透明边角仍会拦截下方桌面点击（点击穿透暂未实现）
- `alwaysOnTop` 无法悬浮在 macOS 原生全屏应用之上
- 剪贴板云同步只同步文本；图片条目依赖本地文件，不同步
- 图库暂不支持 HEIC / RAW；文档管理只索引常见办公与文本格式
- 扫描版 PDF 无文本层，文档转换会返回 `needs_ocr`，需外部 OCR
- Windows 暂不支持一键切换深浅模式与静音
- 磁盘永久删除不可恢复；默认使用系统回收站，执行前必须经过复核清单
- 自动更新依赖 Tauri 签名私钥，私钥丢失将无法再向存量版本推送更新

---

## 第三方许可

依赖清单与许可见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。

依赖（`Cargo.toml` / `package.json`）变更后应重新生成许可清单：

```bash
npm run licenses:generate
```

> 该 npm script 指向 `scripts/gen-third-party-licenses.mjs`。若仓库中暂时缺少该脚本，需要先补回生成脚本，
> 或按 `Cargo.lock` / `package-lock.json` 手工更新清单，避免许可信息与依赖不一致。
