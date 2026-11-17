# 架构设计

本文说明 Desk Pet 的运行时结构、窗口模型、启动时序、前后端分层、事件与数据流，以及几个容易踩坑的设计决策。
命令、事件、表结构的**清单式参考**见 [backend.md](backend.md)。

## 1. 总览

Desk Pet 是**单进程、多 WebView 窗口**的 Tauri 2 应用：

- 一个 Rust 主进程，持有 SQLite 连接、后台任务与系统资源；
- 一个常驻的主窗口 `main`，承载「精灵 + 面板」两种形态；
- 若干按需创建的独立窗口（磁盘分析、图库、文档、接口请求、文本对比、悬浮监控条、更新小窗）；
- 一个独立的 `notify` 窗口，使用独立 HTML 入口 `notify.html` 渲染番茄钟提醒浮层。

```text
┌──────────────────────────── Rust 主进程 ────────────────────────────┐
│ Tauri Builder                                                       │
│  ├─ plugins: single-instance / notification / http / autostart /    │
│  │            dialog / updater / process / global-shortcut          │
│  ├─ invoke_handler: tauri_cmds::*                                   │
│  ├─ img:// 自定义协议（图库缩略图 / 原图）                          │
│  └─ setup: Store / AppState / 托盘 / 剪贴板轮询 / AI 用量 / MCP      │
└───────────────┬───────────────────────────────┬─────────────────────┘
                │ IPC (invoke/event)            │ HTTP / SSE / gRPC
┌───────────────▼───────────────────┐   ┌───────▼─────────────────────┐
│ WebView: main                     │   │ hub.htui.cc                 │
│  App.vue → PanelContainer + Pet   │   │  ├─ 账户 / 收藏 / 剪贴板    │
│ WebView: analyzer/images/docs/... │   │  ├─ 更新清单                │
│  ?view=xxx → 独立根组件           │   │  └─ channel.v1 gRPC         │
│ WebView: notify (notify.html)     │   └─────────────────────────────┘
└───────────────────────────────────┘
```

设计原则：

1. **前端只通过 `src/api` 访问后端**，不直接散落 `invoke` 字符串。唯一例外是更新流程里的
   `updater_open`（`useUpdater.ts` / `useChannel.ts` 直接调用窗口命令，避免与更新器封装形成依赖环）。
2. **Rust 命令是薄壳**：参数校验、任务编排、事件转发；业务逻辑尽量下沉到 `deskpet-*` crate。
3. **重计算不占主线程**：窗口操作走 `run_on_main_thread`，文件扫描 / 图片解码 / 网络请求走
   `spawn_blocking` 或专用线程。
4. **数据与 UI 解耦**：SQLite 是唯一持久化来源，前端 Pinia 只做内存缓存与乐观更新。

## 2. 窗口模型

### 2.1 主窗口：精灵与面板二合一

`main` 窗口在 `tauri.conf.json` 中初始为 72×72、透明、无边框、始终置顶、`visible: false`。
前端 `App.vue` 根据 `ui.expanded` 在两种形态间切换：

| 形态 | 尺寸 | 关键行为 |
| --- | --- | --- |
| 折叠（精灵） | `petWindowSize(sprite)`，最小 72 | 不可缩放；单击展开；拖拽移动；下方可显示时钟 / AI 用量 |
| 展开（面板） | 默认 388×588，最小 300×420 | 可缩放；记住用户手动调整的尺寸；超出屏幕时自动钳制 |

几何控制集中在 `src/lib/windowCtl.ts`：

- `initWindowPosition`：启动恢复位置；无记录则落在主显示器右下角；精灵尺寸大于 72 时同步放大窗口。
- `expandWindow`：记录精灵位置 → 放大到面板尺寸 → 钳制到显示器内 → 解除最小尺寸 → 允许缩放 → 主动取焦点。
- `collapseWindow`：若面板被拖动过则就地收起（精灵在面板左上角），否则恢复展开前的精灵位置。
- `syncPetWindowSize`：精灵大小变化时以「视觉中心」为锚点调整折叠窗口尺寸。
- `panelClearance`：根据精灵大小计算面板顶边与头部左内边距，避免精灵盖住标题与导航。
- `startWindowResize`：非 macOS 走原生 `startResizeDragging`；macOS 因 tao 未实现该能力，改为轮询原生
  `pointer_state` 手动驱动窗口几何。

> 主窗口使用单窗口 resize 而不是「精灵窗口 + 面板窗口」，避免多窗口焦点、层级、位置同步和动画一致性问题。

### 2.2 独立窗口

独立窗口统一由 Rust 侧 `WebviewWindowBuilder` 创建，复用同一个 `index.html`，通过 query 参数选择根组件：

| label | URL | 根组件 | 创建命令 |
| --- | --- | --- | --- |
| `analyzer` | `index.html?view=analyzer` | `AnalyzerApp.vue` | `disk_analyzer_open` |
| `images` | `index.html?view=images[&tab=compress]` | `ImagesApp.vue` | `images_open` |
| `docs` | `index.html?view=docs` | `DocsApp.vue` | `docs_open` |
| `http` | `index.html?view=http` | `HttpApp.vue` | `http_open` |
| `textdiff` | `index.html?view=textdiff` | `TextDiffApp.vue` | `textdiff_open` |
| `monitorbar` | `index.html?view=monitorbar` | `FloatingMonitorApp.vue` | `monitorbar_toggle` |
| `updater` | `index.html?view=updater` | `UpdaterWindow.vue` | `updater_open` |
| `notify` | `notify.html` | `notify-main.ts` | `tauri.conf.json` 预创建 |

共同行为：

- 无边框、透明、`shadow: true`；macOS 下用 `deskpet_os::window::round_window` 把内容层裁成圆角。
- 窗口已存在时聚焦并 `unminimize`，不重复创建；部分窗口支持事件切换内部视图（如 `images://set-view`）。
- 隐藏 Dock（accessory 模式）下新窗口不会自动前置，创建后显式 `show` + `set_focus`。
- 独立窗口启动时会读取 `settings.json` 同步主题、主题色与语言，不依赖主窗口状态。

> 独立窗口不加载精灵 / 剪贴板 / 托盘逻辑，避免重复轮询与状态分叉。前端入口分流在
> `src/main.ts` 的 `bootstrap()`。

### 2.3 提醒浮层

`notify` 窗口由 `tauri.conf.json` 预创建并强制隐藏，`notify.html` + `src/notify-main.ts` 独立渲染。
它接收 `pomodoro://session-done` 与 `channel://push-notice` 事件，显示倒计时结束或服务端推送消息。

> `notify-main.ts` **禁止导入 `virtual:uno.css`**。否则 Vite 会把 UnoCSS 拆成共享 chunk，在
> `index.html` 中先于 Element Plus 的样式加载，导致主窗口工具类被 EP 反超（详见
> [development.md](development.md#54-css-加载顺序关键)）。

## 3. 启动时序

### 3.1 Rust 侧（`src-tauri/src/lib.rs`）

1. 注册插件：单实例、通知、HTTP、自启动、文件对话框、更新器、进程。
2. 注册全部 Tauri 命令（108 个，清单见 [backend.md](backend.md)）。
3. 注册 `img://` 异步协议：在主线程回调中只取 URI，实际解码 / 读盘丢到阻塞线程池。
4. `setup`：
   1. 解析并创建应用数据目录；
   2. `Store::open`：打开 SQLite（WAL）、建表、幂等补列、迁移旧 JSON、清理历史文档索引与孤儿图库条目、
      迁移旧便签；
   3. 把 `Store` 放入 `AppState`；
   4. macOS 下裁剪 `main` / `notify` 窗口内容圆角；
   5. 读取设置，迁移旧版聊天历史到 SQLite；
   6. 根据设置初始化 Dock 可见性（仅 macOS）与置顶状态；
   7. 强制隐藏 `notify` 窗口；
   8. 创建托盘；
   9. 启动剪贴板轮询线程；
   10. 启动 AI 用量后台轮询（每 5 分钟增量扫描）；
   11. 静默连接启用的 MCP 服务器；
   12. 监听 `i18n://locale-changed` 与 `app://name-changed` 重建托盘；
   13. 注册全局快捷键 `CmdOrCtrl+Shift+P`。
5. `run` 进入事件循环。

### 3.2 前端主窗口（`src/App.vue`）

1. `main.ts` 读取 `?view=`，选择根组件；主窗口挂载 `App.vue`。
2. `App.vue` 在 `onMounted`：
   - 等首帧绘制（双 `requestAnimationFrame`，400ms 超时兜底）后 `win.show()`，避免透明窗首帧灰底闪现；
   - 初始化剪贴板、启动器、收藏 store；
   - 启动更新检查与周期检查；
   - `config.init()`：读取设置、恢复窗口位置、应用主题与语言；
   - 注册窗口移动 / 缩放监听；
   - 设置加载完成后触发收藏 / 剪贴板静默拉取；
   - 启动 gRPC 实时通道并注册事件监听。
3. `watch(ui.expanded)` 驱动窗口展开 / 收起，并在收起后显式持久化位置。
4. `watch(config.settings.sprite_size)` 在折叠态同步窗口尺寸（`petGeomReady` 防止与启动恢复竞态）。

### 3.3 独立窗口

独立根组件在 `onMounted` 中：

1. 读取 `settings.json`，应用主题、主题色、语言；
2. 注册窗口缩放（`startWindowResize`，部分窗口有独立最小尺寸）；
3. 注册领域事件监听（扫描进度、缩略图就绪、监控开关等）；
4. 首屏加载数据。

## 4. 分层与模块

### 4.1 前端

| 目录 | 职责 | 约定 |
| --- | --- | --- |
| `src/views/` | 面板内的 8 个页面 | 根节点使用 `view-root`，滚动使用 `view-scroll` |
| `src/*App.vue` | 独立窗口根组件 | 自行同步外观与窗口 resize |
| `src/components/toolbox/` | 工具注册表与工具组件 | 新工具只需改 `registry.ts` + `tools/` + i18n |
| `src/components/pet/` | 精灵渲染、拖拽、时钟 / 徽标 | 位置与尺寸由 `windowCtl` 驱动 |
| `src/components/layout/` | 面板容器、标签导航、头部拖动 | 标签切换只改 `ui.activeView` |
| `src/stores/` | Pinia 状态 | 配置、剪贴板、收藏、启动器、番茄钟、AI 用量、UI |
| `src/composables/` | 有副作用的组合逻辑 | 云同步、gRPC 通道、拖拽、更新器 |
| `src/api/` | `invoke` / HTTP / 事件封装 | 命名与 Rust 命令一一对应，统一返回类型化 Promise |
| `src/lib/` | 纯逻辑与基础设施 | 窗口控制、主题、stash 格式、AI 工具、日志、toast |
| `src/types/` | 前后端共享类型 | Rust `serde` 字段保持 `snake_case`，前端不擅自 camelCase 化 |

### 4.2 Rust

| 目录 | 职责 |
| --- | --- |
| `src-tauri/src/lib.rs` | Builder、插件、命令注册、启动时序 |
| `src-tauri/src/tauri_cmds/` | 按域拆分的命令薄壳 |
| `src-tauri/src/channel.rs` | gRPC 长连接运行时：Hello、心跳、重连、任务、推送 |
| `src-tauri/src/mcp.rs` | MCP 客户端：连接、工具缓存、调用、断开 |
| `src-tauri/src/ai_usage.rs` | Claude Code / Codex CLI / OpenCode 日志增量解析 |
| `src-tauri/src/tray.rs` | 托盘菜单、显隐、国际化重建 |
| `src-tauri/src/state.rs` | `AppState`：Store、剪贴板缓存、通道、MCP、任务取消标志、监控采样器 |
| `src-tauri/src/paths.rs` | 应用数据目录解析与创建 |

### 4.3 可复用 crate

| crate | 依赖 Tauri | 职责 |
| --- | --- | --- |
| `deskpet-store` | 否 | SQLite 持久化、设置读写、旧数据迁移、头像、图库 / 文档 / AI 用量数据层 |
| `deskpet-os` | 否 | 应用启动、快捷操作、剪贴板、文件落盘、图片压缩 / 水印、系统监控、设备信息、macOS 菜单栏 / 窗口圆角 |
| `deskpet-disk` | 否 | 磁盘扫描、分配大小计量、分类、删除护栏、treemap 布局、insights |
| `channel-client` | 否 | gRPC 双向流客户端；proto 副本 + 已提交的生成代码，构建不依赖 protoc |

> 业务逻辑下沉到不依赖 Tauri 的 crate，是这套工程能写单元测试与集成测试的关键。

## 5. 状态与并发

`AppState` 在主进程内单例，通过 `app.state::<AppState>()` 获取：

```rust
pub struct AppState {
    pub clipboard: Mutex<Vec<ClipEntry>>,        // 剪贴板内存缓存
    pub store: Mutex<Store>,                     // SQLite 连接
    pub channel: Mutex<Option<ChannelHandle>>,   // gRPC 通道发送端
    pub mcp: Mutex<HashMap<String, McpServerHandle>>,
    pub image_tasks: Mutex<HashMap<i64, Arc<AtomicBool>>>, // 图库扫描 / 去重取消标志
    pub doc_tasks: Mutex<HashMap<i64, Arc<AtomicBool>>>,   // 文档扫描取消标志
    pub monitor: Mutex<Monitor>,                 // 监控采样器（跨调用保存计数）
}
```

并发约定：

- **SQLite 串行访问**：所有数据库操作在 `state.store.lock()` 内完成；耗时扫描不要在持锁期间执行。
- **主线程只做窗口操作**：`WebviewWindowBuilder`、`set_dock_visibility` 等通过
  `app.run_on_main_thread` 调度；CPU 采样、图片解码、文件扫描走 `spawn_blocking`。
- **取消标志**：长任务（图库扫描、去重、文档扫描）写入 `AtomicBool`，命令侧 `*_task_cancel` 置位。
- **序号守卫**：前端对可能乱序返回的请求（AI 用量、监控采样、删除计划）使用递增序号丢弃过期响应。

## 6. 事件

Rust → 前端的 Tauri 事件按域命名，前端在对应 store / 根组件注册监听。

| 事件 | 方向 | 载荷 / 用途 |
| --- | --- | --- |
| `clipboard://changed` | Rust → 全部窗口 | 剪贴板历史变化，前端刷新列表 |
| `ai_usage://updated` | Rust → 主窗口 | AI 用量扫描完成，刷新徽标 / 工具页 |
| `docs://scan-progress` | Rust → docs | 文档扫描进度 |
| `images://scan-progress` | Rust → images | 图库扫描进度 |
| `images://thumb-ready` | Rust → images | 单张缩略图生成完成 |
| `images://dedup-progress` | Rust → images | 去重任务进度 |
| `images://set-view` | Rust → images | 已打开的图库窗口切换内部视图 |
| `monitorbar://state` | Rust → 全部窗口 | 悬浮监控条开关状态 |
| `monitorbar://click-through` | 主窗口 → 悬浮条 | 鼠标穿透开关 |
| `monitorbar://sections` | 主窗口 → 悬浮条 | CPU / 内存 / 网速分区开关 |
| `monitorbar://refresh` | 主窗口 → 悬浮条 | 刷新间隔变更 |
| `channel://hello-ack` | Rust → 主窗口 | 通道握手结果与待处理任务 |
| `channel://task-assign` | Rust → 主窗口 | 服务端下发任务 |
| `channel://task-cancel` | Rust → 主窗口 | 任务取消 |
| `channel://push` | Rust → 主窗口 | 服务端推送消息 |
| `channel://push-notice` | Rust → notify | 推送提醒浮层 |
| `pomodoro://session-done` | 主窗口 → notify | 番茄钟阶段完成 |
| `i18n://locale-changed` | 前端 → Rust | 语言切换，重建托盘 |
| `app://name-changed` | 前端 → Rust | 应用名变更，重建托盘提示 |

> 磁盘清理 / 分析的扫描进度**不走 Tauri 事件**，而是本地 `127.0.0.1:<随机端口>/cleanup/events/<job>`
> 的 SSE。原因是扫描事件高频、体积大，SSE 有天然背压与断线语义，且 `axum` 实现简单；服务启动时校验
> `Origin`，只允许 Tauri / localhost 来源。

## 7. 关键数据流

### 7.1 剪贴板捕获

```text
系统剪贴板
  └─ Rust 轮询线程（每秒，arboard）
       ├─ 文本：内容去重 → 更新 copy_count / updated_at
       ├─ 图片：RGBA FNV-1a 签名去重 → 写入 clip_images/<id>.png
       ├─ 更新 AppState.clipboard + SQLite（全量快照）
       └─ emit clipboard://changed
            └─ 前端 clipboard store 重新 getClipboardHistory
```

- 文本与图片统一存在 `ClipEntry`，`kind = "text" | "image"`；图片条目的 `text` 字段存 PNG 文件名。
- 剪贴板历史上限在 Rust 侧维护；删除条目时同步删除图片文件。
- 云同步只推送 `kind != "image"` 的文本条目。

### 7.2 收藏 / 剪贴板云同步

```text
启动 / 打开页面 / 手动 / 5 分钟节流
  ├─ push：本地全量 upsert → POST /api/cms/{favorite|clipboard}/batch
  └─ pull：游标分页 → POST /api/cms/{favorite|clipboard}/pull
        ├─ 本地 upsert（LWW：本地 update_time 更新则保留本地）
        ├─ 应用 deleted 墓碑
        └─ 保存新的 serverNow 游标
```

- 鉴权统一使用 `settings.auth_token`，请求头为裸 token。
- 收藏拉取采用**全量游标分页**而不是按 `updatedAfter` 增量，避免多设备时钟偏差漏数据。
- 服务端时间统一用 `YYYY-MM-DD HH:MM:SS`（UTC），前端在 `lib/stash.ts` 做 ISO 归一。
- 接口契约见 [clipboard-sync-api.md](clipboard-sync-api.md)。

### 7.3 AI 对话与工具调用

```text
用户输入
  └─ Chat.vue 组装 messages + tools（内置工具 + 已连接 MCP 工具）
       └─ POST {base_url}/chat/completions（流式）
            ├─ 纯文本：逐块追加渲染
            └─ tool_calls：按 index 拼接 → 路由执行
                 ├─ 内置工具：前端直接执行（启动应用 / 切换视图 / 番茄钟）
                 ├─ MCP 工具：invoke mcp_call_tool → rmcp 调用外部服务器（执行前确认）
                 └─ 结果作为 role:"tool" 消息回填，继续对话
       └─ 消息落 SQLite chat_message
```

- agent 循环上限 8 轮，历史窗口 60 条，防止无限增长。
- 模型不支持 function-calling 时，首次 4xx 后降级为普通对话。
- MCP 服务器连接句柄保存在 `AppState.mcp`；断开时取消令牌并优雅关闭子进程。

### 7.4 磁盘清理 / 分析

```text
前端选择根目录
  └─ disk_cleanup_scan / disk_analyze_scan
       └─ deskpet-disk 扫描（分配大小计量、硬链接去重、分类、护栏）
            ├─ 返回本地 SSE URL
            └─ 前端 EventSource 接收 Started/Progress/Found/ScanDone...
                 ├─ disk_*_plan：生成复核清单（大小、文件数、风险）
                 └─ disk_*_delete：按 trash / permanent 执行，持续推送进度
```

- 分析窗口树缓存按 `scanId` LRU 保留 2 份，10 分钟 TTL；窗口关闭可显式释放。
- 删除前必须生成计划；永久删除不可恢复，默认回收站。
- `deskpet-disk` 的删除护栏包含路径规范化、符号链接检查、家目录 / 整仓保护等。

### 7.5 图库

```text
添加图库目录
  └─ images_scan_start（后台线程）
       ├─ walkdir 递归扫描，size + mtime 未变则跳过
       ├─ 解析图片尺寸 / EXIF 拍摄时间 / 相机
       ├─ 同尺寸文件计算 blake3，用于精确去重
       ├─ 缩略图落盘 image_thumbs/ + SQLite 索引
       └─ emit images://scan-progress / images://thumb-ready
前端展示
  └─ img://localhost/{thumb|full}/{id}?v=<mtime>
       └─ Rust 协议处理：按 id 反查 DB，读盘 / 解码后返回，WebView 按 immutable 缓存
```

- Windows 上自定义协议以 `http://img.localhost/...` 代理。
- 根目录（`/`、`C:\`）按全盘扫描处理，跳过系统目录。
- 目录成员按路径前缀判定，嵌套目录共享同一份索引。

### 7.6 文本对比

`text_diff` 命令在 Rust 侧完成：

1. `similar` 做行级 diff，再对变更行做词级 diff；
2. `tree-sitter` 按语言解析并做语法高亮，输出 token 类型；
3. 返回 `rows`（每行的 old/new 行号与 segments）+ 增删统计，前端只负责渲染。

## 8. 外部协议

| 协议 | 位置 | 说明 |
| --- | --- | --- |
| 账户 API | `src/api/auth.ts` | `https://hub.htui.cc/api/pas/user`，图形码 / 滑块 / 登录注册 |
| 收藏同步 | `src/composables/useStashCloud.ts` | `/api/cms/favorite/*`，裸 token |
| 剪贴板同步 | `src/composables/useClipboardCloud.ts` | `/api/cms/clipboard/*`，裸 token，仅文本 |
| 实时通道 | `src-tauri/src/channel.rs` | gRPC `channel.v1.ChannelService/Open`，双向流 |
| MCP | `src-tauri/src/mcp.rs` | `rmcp` 0.8，stdio 子进程 / Streamable HTTP |
| 自动更新 | `tauri.conf.json` + `useUpdater.ts` | `tauri-plugin-updater`，签名校验 |
| 发布上传 | `.github/workflows/hub-upload.yml` | `POST /api/core/release/upload`，multipart |

## 9. 平台差异与踩坑记录

### macOS

- 透明窗口依赖私有 API：`tauri.conf.json` 的 `macOSPrivateApi` + Cargo 的 `macos-private-api` feature，
  因此**不能上架 Mac App Store**。
- 无边框透明窗的阴影会被方形窗口边界硬裁，底部出现直角半透明带；解决方式是用
  `round_window` 把窗口内容层裁成圆角（主窗 24，通知 20，独立窗 12，更新小窗 14，监控条 8）。
- accessory（隐藏 Dock）模式下点击不会自动激活应用，键盘输入失效；面板展开时主动 `setFocus()`，
  面板内容 `pointerdown` 时也补一次焦点。
- `tao` 未实现 `startResizeDragging`，且错误被运行时吞掉；macOS 的窗口缩放手柄改为轮询原生指针状态手动驱动。
- 标题栏拖动使用 4px 位移阈值：`acceptFirstMouse` + accessory 下，点击激活与原生拖拽会同时触发，
  立即拖拽会携带错误坐标导致面板瞬移。
- WKWebView 在窗口未显示时可能暂停 `requestAnimationFrame`，主窗口显示逻辑必须带超时兜底。

### 通用

- WebView 默认右键菜单（刷新、检查元素、系统编辑菜单）在 `main.ts` 中被全局禁用；托盘右键菜单由
  Rust 侧提供，不受影响。
- 展开 / 收起是程序化 `setPosition` + `setSize`，系统不保证触发 `onMoved`，因此收起后需显式持久化位置。
- 连续拖动精灵大小滑杆时，窗口操作会串行化，只对末次值生效。
- 番茄钟剩余时间基于时间戳计算，避免 WebView 定时器后台节流导致漂移。
- 监控条与主窗口通过事件双向同步开关和刷新间隔，避免两套状态分叉。

## 10. 扩展点

| 需求 | 扩展位置 |
| --- | --- |
| 新增面板标签页 | `stores/ui.ts` 的 `PanelView` + `PanelContainer.vue` 的 `tabs` + 新建 `views/` + i18n |
| 新增工具箱工具 | `components/toolbox/tools/` + `registry.ts` + i18n |
| 新增独立窗口 | `index.html?view=xxx` 根组件 + `main.ts` 分支 + Rust 创建命令 + `capabilities/default.json` |
| 新增 Tauri 命令 | `tauri_cmds/<域>.rs` + `lib.rs` 注册 + `api/<域>.ts` 封装 |
| 新增数据表 | `deskpet-store/src/store.rs` 的 `DDL` + 幂等迁移 + 数据访问方法 + 文档 |
| 新增外部协议 | 对应 `api/` / `composables/` / Rust 模块 + 错误处理与超时 |
| 新增后台任务 | `AppState` 存取消标志 / 句柄 + `setup` 启动 + 事件推送进度 |

具体步骤与代码约定见 [development.md](development.md)。
