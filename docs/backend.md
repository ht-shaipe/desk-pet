# 后端参考

本文是 Rust 侧与持久化层的清单式参考：workspace 结构、`AppState`、全部 Tauri 命令、事件、SQLite 表、
`settings.json` 字段、后台任务与外部协议。设计动机与数据流见 [architecture.md](architecture.md)。

## 1. Workspace 结构

```text
Cargo.toml                 # workspace 根：统一依赖版本 + release profile
crates/
  deskpet-store/           # SQLite、设置、迁移、头像、图库 / 文档 / AI 用量数据层（不依赖 Tauri）
  deskpet-os/              # 应用启动、快捷操作、剪贴板、文件、图片、监控、菜单栏、窗口圆角
  deskpet-disk/            # 磁盘扫描、大小计量、分类、删除护栏、treemap、insights
  channel-client/          # gRPC 双向流客户端（proto 副本 + 已提交生成代码）
src-tauri/
  src/lib.rs               # Builder、插件、命令注册、启动时序
  src/tauri_cmds/          # 按域拆分的命令薄壳
  src/state.rs             # AppState
  src/paths.rs             # 应用数据目录
  src/channel.rs           # gRPC 通道运行时
  src/mcp.rs               # MCP 客户端
  src/ai_usage.rs          # Claude Code / Codex CLI / OpenCode 日志增量解析
  src/ai_usage_pricing.rs  # 模型价格表与成本估算
  src/tray.rs              # 托盘菜单
```

### 1.1 crate 职责

| crate | 关键模块 | 职责 |
| --- | --- | --- |
| `deskpet-store` | `store.rs`、`settings.rs`、`avatar.rs`、`gallery.rs`、`docs.rs`、`ai_usage.rs`、`opencode.rs` | 打开 SQLite（WAL）、建表 / 补列 / 迁移、设置读写、头像落盘、图库 / 文档 / 用量查询 |
| `deskpet-os` | `apps.rs`、`launch.rs`、`actions.rs`、`clipboard.rs`、`files.rs`、`icons.rs`、`imaging.rs`、`watermark.rs`、`monitor.rs`、`menubar.rs`、`window.rs`、`pointer.rs`、`device.rs` | 系统集成与可独立测试的文件 / 图片 / 采样逻辑 |
| `deskpet-disk` | `scan.rs`、`size.rs`、`classify.rs`、`tree.rs`、`removal.rs`、`treemap.rs`、`insights.rs`、`space.rs`、`pool.rs` | 扫描、硬链接去重、分类、风险与删除护栏、treemap 布局 |
| `channel-client` | `client.rs`、`channel.v1.rs`、`proto/channel.proto` | tonic 客户端：Hello、心跳、重连、任务确认 / 进度、用户资料上报 |

`deskpet-*` 不依赖 Tauri，是补充单元测试的首选位置。协议契约以 `crates/channel-client/proto/channel.proto`
为准；`src/channel.v1.rs` 是已提交的生成代码，构建不依赖 `protoc`。

### 1.2 release profile

workspace 根 `Cargo.toml` 对 release 开启 `lto = true`、`codegen-units = 1`、`opt-level = 3`、
`panic = "abort"`、`strip = true`，用于压缩安装包体积。

## 2. AppState

`src-tauri/src/state.rs`，单例，通过 `app.state::<AppState>()` 获取：

| 字段 | 类型 | 用途 |
| --- | --- | --- |
| `clipboard` | `Mutex<Vec<ClipEntry>>` | 剪贴板内存历史；SQLite 为持久化来源 |
| `store` | `Mutex<Store>` | 唯一 SQLite 连接；所有 DB 操作串行 |
| `channel` | `Mutex<Option<ChannelHandle>>` | gRPC 通道发送端与取消信号 |
| `mcp` | `Mutex<HashMap<String, McpServerHandle>>` | 已连接 MCP 服务器，key 为配置 id |
| `image_tasks` | `Mutex<HashMap<i64, Arc<AtomicBool>>>` | 图库扫描 key 为 `dir_id`，去重为 `-1` |
| `doc_tasks` | `Mutex<HashMap<i64, Arc<AtomicBool>>>` | 文档扫描取消标志，key 为 `doc_dir.id` |
| `monitor` | `Mutex<Monitor>` | 跨调用保存 CPU / 网络采样计数，用于算速率 |

并发规则：

- 不要在持有 `store` 锁时做文件 IO / 图片解码 / 网络请求。
- 窗口、Dock、菜单栏操作走 `app.run_on_main_thread`。
- 扫描、压缩、转换、日志解析走 `spawn_blocking` 或专用 `std::thread`。
- 长任务必须注册取消标志，并在结束时移除，否则下一次任务会被误判为「进行中」。

## 3. Tauri 命令目录

共 **108** 个命令，全部在 `src-tauri/src/lib.rs` 的 `generate_handler!` 注册。前端封装在 `src/api/*.ts`。
参数列按 Rust 侧 `snake_case` 展示；前端 `invoke` 的 key 用 camelCase（`data_url` → `dataUrl`）。

### 3.1 设置与头像（`settings_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `get_settings` | — | `AppSettings` | 读取 `settings.json`，不存在或损坏返回默认值 |
| `save_settings` | `settings` | `()` | 整体写回；前端快照缺 `device_id` 时保留磁盘值 |
| `set_pet_avatar` | `src` | `String` | 拷贝用户选择图片到 app data，返回新路径 |
| `reset_pet_avatar` | — | `()` | 删除自定义头像 |

### 3.2 启动器（`launcher_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `get_launcher` | — | `LauncherConfig` | 读取启动项列表 |
| `save_launcher` | `config` | `()` | 全量保存 |
| `launch_item` | `item` | `()` | 按 `app` / `url` / `cmd` 执行 |
| `get_app_icon` | `item` | `Option<String>` | 提取应用图标 PNG，缓存到 `icons/` |
| `get_app_icon_by_value` | `value` | `Option<String>` | 按路径 / 名称提取图标，供选择列表预览 |
| `list_apps` | — | `Vec<AppEntry>` | 枚举已安装应用 |

### 3.3 剪贴板（`clipboard_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `get_clipboard_history` | — | `Vec<ClipEntry>` | 返回内存快照，倒序 |
| `clear_clipboard_history` | — | `()` | 清空并删除图片文件 |
| `delete_clipboard_entry` | `id` | `()` | 删除单条，图片条目同步删文件 |
| `delete_clipboard_entries` | `ids` | `()` | 批量删除（按类型 / 日期清理） |
| `merge_clipboard_entries` | `entries`、`deleted` | `usize` | 云端拉取合并；图片条目跳过 |
| `copy_to_clipboard` | `text` | `()` | 写系统剪贴板 |
| `copy_image_to_clipboard` | `filename` | `()` | 按 PNG 文件名写图片到剪贴板 |
| `get_clip_image_dir` | — | `String` | 剪贴板图片目录绝对路径 |

剪贴板常量（`clipboard_cmds.rs`）：最多 500 条、文本最多 4000 字符、轮询 1 秒、图片上限 12 MB。
图片用 RGBA 字节 FNV-1a 64 签名去重。

### 3.4 收藏（`favorite_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `get_favorites` | — | `Vec<Favorite>` | 全量读取 |
| `upsert_favorites` | `items` | `usize` | 导入 / 云拉取 / 新增共用 |
| `delete_favorites` | `urls` | `usize` | 批量删除 |
| `clear_favorites` | — | `()` | 清空 |
| `read_favorite_import_file` | `path` | `String` | 读取用户选择的 JSON，上限 8 MB |
| `write_favorite_export_file` | `path`、`content` | `()` | 导出 JSON |

### 3.5 聊天记录（`chat_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `get_chat_messages` | — | `Vec<ChatEntry>` | 按时间正序 |
| `add_chat_message` | `role`、`content`、`tool_calls?`、`tool_call_id?` | `ChatEntry` | 支持工具调用消息 |
| `delete_chat_message` | `id` | `()` | 删除单条 |
| `delete_chat_messages_from` | `id` | `usize` | 删除该条及之后，用于重新执行 |
| `clear_chat_messages` | — | `()` | 清空 |

### 3.6 系统与指针（`system_cmds.rs`、`pointer_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `get_platform` | — | `String` | `std::env::consts::OS`（`macos` / `windows`） |
| `quick_action` | `action` | `()` | 深浅模式、静音、锁屏、睡眠、打开系统设置页 |
| `set_show_dock` | `visible` | `()` | 仅 macOS；切换后重新拉起主窗口 |
| `debug_log` | `scope`、`message` | `String` | 追加到 `debug.log`，返回文件路径 |
| `pointer_state` | — | `PointerState` | 全局光标坐标 + 左键状态，macOS 手动 resize 用 |

### 3.7 工具箱（`toolbox_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `get_clipboard_image` | — | `Option<ImagePayload>` | 读取系统剪贴板图片 |
| `set_clipboard_image` | `payload` | `()` | RGBA 图片写入剪贴板 |
| `save_qr_png` | `data_url` | `String` | 保存二维码 PNG 到 `qrcodes/` 并定位文件 |
| `save_image` | `data_url` | `String` | 保存任意图片到 `images/` 并定位 |
| `list_image_files` | `dir` | `Vec<ImageFileInfo>` | 仅当前目录、跳过隐藏文件、按名排序 |
| `compress_image_data` | `data_url`、`params` | `CompressImageResult` | 单图预览压缩 |
| `compress_image_file` | `path`、`params`、`out_dir?`、`include_data` | `CompressImageResult` | `out_dir` 为空只估算；否则写 `{out_dir}/compressed/{stem}.min.{ext}` |
| `reveal_path` | `path` | `()` | 文件管理器中定位 |
| `http_request` | `spec` | `HttpResponseData` | Rust 代理发送，绕开 WebView 跨域，回传头 / 耗时 / 体积 |
| `document_convert` | `path` | `DocumentConvertResult` | 任意格式转 Markdown；扫描版 PDF 返回 `needs_ocr` |
| `save_document_markdown` | `path`、`content` | `()` | 保存转换结果 |

`CompressParamsDto`：`quality`、`max_dim`、`format`、`target_kb`、`watermark`（文字 / 图片 / 盲水印）。
`document_convert` 支持 doc/docx、ppt 系、xls 系、odt/ods/odp、rtf、epub、csv、pdf 等 12 种格式，
按内容签名探测，扩展名只作兜底。

### 3.8 磁盘清理（`cleanup_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `disk_list_volumes` | — | `Vec<VolumeDto>` | 过滤伪文件系统与系统私有挂载 |
| `disk_open_full_disk_access` | — | `()` | macOS 打开「完全磁盘访问」设置页 |
| `disk_cleanup_usage` | `root` | `DiskUsageInfo` | 所在卷总容量 / 可用空间 |
| `disk_cleanup_scan` | `root`、`log_days` | `CleanupStream` | 规则化候选扫描，返回 SSE 地址 |
| `disk_cleanup_plan` | `scan_job_id`、`paths` | `PlanResponse` | 复核清单：targets / covered / blocked / reclaim |
| `disk_cleanup_delete` | `scan_job_id`、`paths`、`mode` | `CleanupStream` | `trash` / `permanent`，返回新 SSE 地址 |
| `disk_cleanup_cancel` | `job_id` | `()` | 幂等取消扫描或删除 |

### 3.9 磁盘分析（`analyze_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `disk_analyzer_open` | — | `bool` | 打开 / 聚焦 `analyzer` 窗口，返回是否新建 |
| `disk_analyze_scan` | `root` | `CleanupStream` | 全树扫描，`scan_id` 即 `job_id` |
| `disk_analyze_layout` | `scan_id`、`path`、`width`、`height`、`max_depth`、`metric`、`filter?` | `LayoutResult` | treemap tile；`metric` 为 `bytes` / `files` |
| `disk_analyze_insights` | `scan_id` | `Vec<Insight>` | 「值得看看」大目录 |
| `disk_analyze_marks` | `scan_id`、`paths` | `Vec<MarkInfo>` | 标记项当前大小 / 是否存在 |
| `disk_analyze_plan` | `scan_id`、`paths` | `PlanResponse` | 标记项复核清单 + 风险 |
| `disk_analyze_delete` | `scan_id`、`paths`、`mode` | `CleanupStream` | 删除标记项并推送进度 |
| `disk_analyze_close` | `scan_id` | `()` | 释放树缓存 |

树缓存按 `scanId` LRU 保留 2 份、10 分钟 TTL。

### 3.10 文档管理（`docs_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `docs_open` | — | `bool` | 打开 / 聚焦 `docs` 窗口 |
| `docs_add_dir` | `path` | `DocDir` | 添加文档目录 |
| `docs_list_dirs` | — | `Vec<DocDir>` | 列出目录 |
| `docs_remove_dir` | `id` | `u64` | 删除目录索引，返回清理条目数 |
| `docs_scan_start` | `dir_id` | `()` | 后台增量扫描，进度走 `docs://scan-progress` |
| `docs_task_cancel` | `dir_id` | `()` | 取消扫描 |
| `docs_query` | `query` | `DocPage` | 分页 / 搜索 / 扩展名 / 排序 |
| `docs_list_exts` | — | `Vec<DocExtStat>` | 侧栏类型统计 |
| `docs_delete_item` | `id` | `()` | 移废纸篓 + 清索引；文件已不存在视为成功 |

`DocQueryDto`：`dir_id?`、`search`、`ext?`、`sort_by?`、`sort_order?`、`limit`、`offset`。

### 3.11 图库（`images_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `images_open` | `view?` | `bool` | 打开 / 聚焦 `images`；已开时用 `images://set-view` 切视图 |
| `images_open_path` | `path` | `()` | 系统默认程序打开文件 |
| `images_add_dir` | `path` | `ImageDir` | 根路径自动标记为全盘扫描 |
| `images_list_dirs` | — | `Vec<ImageDir>` | 列出图库目录 |
| `images_remove_dir` | `id` | `()` | 删除目录索引 |
| `images_scan_start` | `dir_id` | `()` | 后台增量扫描，进度走 `images://scan-progress` |
| `images_task_cancel` | `task_key` | `()` | 取消扫描 / 去重 |
| `images_query` | `query` | `ImagePage` | 分页 / 目录 / 收藏 / 文档图 / 标签 / 搜索 |
| `images_detail` | `id` | `Option<ImageDetail>` | 条目 + 标签 |
| `images_set_fav` | `id`、`fav` | `()` | 收藏开关 |
| `images_delete_item` | `id` | `()` | 单条移废纸篓 + 清缩略图 |
| `images_list_tags` | — | `Vec<ImageTag>` | 全部标签 |
| `images_item_tag_pairs` | — | `Vec<ItemTagPair>` | 条目-标签关系，缩略图色点用 |
| `images_add_tag` | `name`、`color` | `ImageTag` | 新建标签 |
| `images_rename_tag` | `id`、`name`、`color` | `()` | 重命名 / 改色 |
| `images_delete_tag` | `id` | `()` | 删除标签 |
| `images_set_item_tag` | `item_id`、`tag_id`、`on` | `()` | 绑定 / 解绑 |
| `images_dedup_start` | — | `()` | 对同尺寸且未哈希文件算 blake3 |
| `images_dup_groups` | — | `Vec<ImageDupGroup>` | 重复组 |
| `images_delete_paths` | `paths`、`ids` | `()` | 批量移废纸篓，一次 Finder 调用只响一次 |

`ImageQueryDto`：`dir_id?`、`fav_only`、`doc_only`、`tag_id?`、`search`、`limit`、`offset`。
`img://` 不是命令，而是自定义 URI 协议：`img://localhost/{thumb|full}/{id}?v=<mtime>`，路径按 id 从 DB
反查，杜绝目录遍历；Windows 下走 `http://img.localhost/...`。

### 3.12 系统监控（`monitor_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `monitor_sample` | — | `SystemSample` | CPU / 内存 / 网络上下行 / 磁盘卷 |
| `monitorbar_toggle` | — | `bool` | 打开 / 关闭悬浮条，广播 `monitorbar://state` |
| `monitor_menubar_set` | `sample?`、`show_icon`、`sections?` | `()` | macOS 菜单栏芯片图；`sample = null` 清除 |

`sections` 为 `{ cpu, mem, net }`，与悬浮条共用同一组开关。

### 3.13 AI 用量（`ai_usage_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `ai_usage_refresh` | — | `u64` | 增量扫描 Claude Code / Codex CLI / OpenCode 日志，返回新增事件数 |
| `ai_usage_query` | `from_ms`、`to_ms` | `AiUsageRangeResult` | 聚合 totals / by_tool / by_model / by_project，含估算成本 |

`AiUsageRangeResult.has_any` 区分「未检测到日志」与「该时段无用量」。任一模型缺价时，相关成本为 `null`，
前端显示 `—`。

### 3.14 实时通道（`channel_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `channel_start` | `token`、`user?` | `String` | 建立连接，返回稳定 `device_id`；空 token 为匿名 |
| `channel_stop` | — | `()` | 关闭连接 |
| `channel_report_user` | `user?` | `()` | 登录态补报；空 `user_id` 为显式解绑 |
| `channel_task_ack` | `task_id`、`attempt`、`success`、`message` | `()` | 任务接收确认 / 拒绝 |
| `channel_task_progress` | `task_id`、`status`、`result` | `()` | `3` 执行中 / `4` 成功 / `5` 失败 |

`channel_start` 首次生成并持久化 `device_id`；`save_settings` 不会覆盖它。

### 3.15 MCP（`mcp_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `mcp_connect` | `server_id` | `Vec<McpToolDto>` | 按 `settings.json` 配置连接并缓存工具 |
| `mcp_disconnect` | `server_id` | `()` | 取消令牌并关闭 |
| `mcp_server_tools` | `server_id` | `Vec<McpToolDto>` | 已连接直接返回；启用但未连接时懒连接 |
| `mcp_call_tool` | `server_id`、`tool`、`arguments` | `Value` | `arguments` 为 JSON 对象字符串 |
| `mcp_server_status` | — | `Vec<String>` | 已连接服务器 id |

### 3.16 文本对比与窗口（`diff_cmds.rs`、`textdiff_cmds.rs`、`http_cmds.rs`、`updater_cmds.rs`）

| 命令 | 参数 | 返回 | 说明 |
| --- | --- | --- | --- |
| `text_diff` | `left`、`right`、`language?` | `TextDiffResultDto` | 行级 + 词级 diff + tree-sitter 高亮；单侧上限 2 MB |
| `textdiff_open` | — | `bool` | 打开 / 聚焦 `textdiff` |
| `http_open` | — | `bool` | 打开 / 聚焦 `http` |
| `updater_open` | — | `bool` | 打开 / 聚焦 `updater` 更新小窗 |

`language` 为 `null`、`"plain"` 或未知值时按纯文本处理。

## 4. 事件

Rust → 前端的 Tauri 事件。定向事件用 `emit_to("<label>")`，其余为广播。

| 事件 | 目标 | 载荷 | 触发 |
| --- | --- | --- | --- |
| `clipboard://changed` | 全部 | `()` | 剪贴板新增 / 删除 / 合并后 |
| `ai_usage://updated` | 全部 | `number`（新增事件数） | 每 5 分钟后台扫描有增量 |
| `docs://scan-progress` | `docs` | `{ dirId, scanned, added, updated, removed, done }` | 文档扫描每 20 条 + 结束 |
| `images://scan-progress` | `images` | `{ dirId, scanned, added, updated, removed, thumbsDone, thumbsTotal, done }` | 索引每 20 条、缩略图每 4 张 |
| `images://thumb-ready` | `images` | `{ id }` | 单张缩略图生成完成 |
| `images://dedup-progress` | `images` | `{ total, done, doneFlag }` | 去重每 10 个 + 结束 |
| `images://set-view` | `images` | `string`（如 `compress`） | 窗口已开时切换内部视图 |
| `monitorbar://state` | 全部 | `boolean` | 悬浮条开关（托盘 / 工具页 / 悬浮条关闭共用） |
| `monitorbar://click-through` | `monitorbar` | `boolean` | 主窗口同步鼠标穿透 |
| `monitorbar://sections` | `monitorbar` | `{ cpu, mem, net }` | 分区开关变更 |
| `monitorbar://refresh` | `monitorbar` | `number`（毫秒） | 刷新间隔变更 |
| `channel://hello-ack` | 全部 | `{ success, message, server_time, pending_tasks }` | 通道握手 |
| `channel://task-assign` | 全部 | `{ task_id, task_type, attempt, payload }` | 服务端下发任务 |
| `channel://task-cancel` | 全部 | `{ task_id }` | 任务取消 |
| `channel://push` | 全部 | `{ message_id, message_type, payload, server_time }` | `notify` 窗口创建失败时的回退提示 |
| `channel://push-notice` | `notify` | `{ title, message, time }` | 服务端推送通知 |
| `pomodoro://session-done` | `notify` | `{ title, message, time? }` | 番茄钟阶段完成 |
| `i18n://locale-changed` | Rust | `string` | 语言切换，重建托盘 |
| `app://name-changed` | Rust | `string` | 应用名变更，重建托盘提示 |

### 4.1 磁盘扫描 SSE

磁盘清理 / 分析**不走 Tauri 事件**。`disk_cleanup_scan` / `disk_cleanup_delete` /
`disk_analyze_scan` / `disk_analyze_delete` 返回：

```json
{ "jobId": "...", "url": "http://127.0.0.1:<随机端口>/cleanup/events/<job>" }
```

前端用 `EventSource` 订阅，每帧 `data` 是 JSON，按 `type` 区分：

| `type` | 关键字段 |
| --- | --- |
| `started` | `mode`、`root` |
| `progress` | `dirs`、`files`、`bytes`、`errors`、`current` |
| `found` | `items[]`（候选：`path`、`kind`、`bytes`、`files`、`modified`、`category`、`reclaim`、`risk`、`defaultSelected`） |
| `preview` | `entries[]`、`elapsedMs` |
| `scanDone` | `count`、`bytes`、`files`、`errors`、`truncated`、`complete`、`durationMs` |
| `deleting` | `path`、`bytes`、`mode` |
| `deleted` | `path`、`ok`、`bytes`、`error` |
| `deleteDone` | `removed`、`failed`、`bytes`、`freedBytes`、`freeBefore`、`freeAfter`、`failures[]` |
| `cancelled` | `mode` |
| `failed` | `mode`、`error` |

本地服务只监听 `127.0.0.1`，随机端口；`Origin` 必须是 Tauri / localhost，否则返回 403。SSE 每 10 秒
发 keep-alive；同一个 job 支持断线重连。

## 5. SQLite

数据库文件：`<app_data_dir>/deskpet.db`，WAL 模式，`rusqlite` bundled。DDL 与补列逻辑集中在
`crates/deskpet-store/src/store.rs`。

| 表 | 用途 | 关键列 / 约束 |
| --- | --- | --- |
| `notes` | 旧版独立便签（兼容保留） | `id`、`content`、`color`、`updated_at` |
| `clipboard` | 剪贴板历史 | `id` PK、`text`、`kind`（`text` / `image`）、`copy_count`、`sig`、`updated_at` |
| `launcher` | 启动项 | `id` PK、`name`、`icon`、`item_type`、`value`、`color`、`sort_order` |
| `chat_message` | AI 对话 | 自增 `id`、`role`、`content`、`tool_calls`、`tool_call_id`、`created_at` |
| `favorite` | 收藏 / 便签 | `url` PK、`name`、`description`、`fav_type`、`payload`、`languages`、`stars`、`forks`、`tags`、`category`、`category_name`、`create_time`、`update_time` |
| `image_dir` | 图库目录 | 自增 `id`、`path` UNIQUE、`full_disk`、`added_at`、`last_scan_at` |
| `image_item` | 图库条目 | 自增 `id`、`dir_id`、`path` UNIQUE、`name`、`ext`、`size`、`mtime`、`width`、`height`、`taken_at`、`camera`、`is_fav`、`doc`、`thumb`、`hash` |
| `image_tag` | 图库标签 | 自增 `id`、`name` UNIQUE、`color` |
| `image_item_tag` | 条目-标签关系 | 复合主键 `(item_id, tag_id)` |
| `doc_dir` | 文档目录 | 自增 `id`、`path` UNIQUE、`added_at`、`last_scan_at` |
| `doc_item` | 文档条目 | 自增 `id`、`dir_id`、`path` UNIQUE、`name`、`ext`、`size`、`mtime`、`added_at` |
| `ai_usage_event` | AI 用量事件 | 自增 `id`、`tool`、`ts`、`model`、`project`、`input_tokens`、`output_tokens`、`cache_read`、`cache_write`、`cost_usd`、`dedup_key` UNIQUE |
| `ai_usage_file_state` | 日志增量状态 | 主键 `(path, tool)`、`size`、`mtime`、`parsed_lines`、`last_*` |

索引：`image_item(taken_at DESC, id DESC)`、`image_item(dir_id)`、`image_item(hash)`、
`doc_item(dir_id)`、`doc_item(name)`、`ai_usage_event(ts)`、`ai_usage_event(tool, ts)`。

### 5.1 迁移

启动时 `Store::open` 依次：

1. 建表（`CREATE TABLE IF NOT EXISTS`）。
2. 幂等补列（`ALTER TABLE ... ADD COLUMN`，忽略「已存在」错误）：
   `clipboard.kind/copy_count/sig`、`chat_message.tool_calls/tool_call_id`、`ai_usage_event.cost_usd`。
3. 旧 JSON 迁移：`notes.json`（并入收藏）、`clipboard.json`、`launcher.json`，原文件重命名为 `.json.bak`。
4. 清理历史文档索引与孤儿图库条目。
5. 旧版聊天历史从 `settings.json` 迁入 `chat_message`。

所有迁移必须幂等，重复启动不能丢数据或报错。

## 6. settings.json

位置：`<app_data_dir>/settings.json`。读取失败返回默认值；保存为 pretty JSON。字段全部可选，
前端 `config` store 负责归一化与夹取范围。

| 字段 | 类型 | 默认 / 取值 | 说明 |
| --- | --- | --- | --- |
| `pet_x` / `pet_y` | `number?` | `null` | 精灵位置（物理像素） |
| `always_on_top` | `bool` | `true` | 窗口置顶 |
| `locale` | `string?` | 系统语言 | `zh-CN` / `en` |
| `theme` | `string?` | `auto` | `light` / `dark` / `auto` |
| `sprite_size` | `number?` | `48` | 32–96，前端夹取 |
| `accent_color` | `string?` | 内置蓝 | `#rrggbb` |
| `auth_token` | `string?` | `null` | 云同步 token，裸 `Authorization` 头 |
| `auth_user` | `string?` | `null` | 登录手机号 |
| `device_id` | `string?` | 首次连接生成 | gRPC 通道稳定设备指纹 |
| `pet_avatar` | `string?` | `null` | 自定义头像绝对路径 |
| `app_name` | `string?` | `Desk Pet` | 面板与托盘显示名 |
| `show_dock` | `bool?` | `true` | 仅 macOS |
| `ai_models` | `AiModel[]` | `[]` | `id`、`name`、`base_url`、`api_key`、`model` |
| `ai_active` | `string?` | `null` | 当前模型 id |
| `ai_messages` | `ChatMessage[]` | `[]` | 旧版聊天历史，启动迁移后清空 |
| `favorite_sync` | `bool?` | `false` | 收藏云同步开关 |
| `favorite_last_sync` | `string?` | `null` | 服务端 `serverNow` 游标 |
| `clipboard_sync` | `bool?` | `false` | 剪贴板云同步开关 |
| `clipboard_last_sync` | `string?` | `null` | 服务端 `serverNow` 游标 |
| `pomo_focus_min` | `number?` | `25` | 1–120 |
| `pomo_short_min` | `number?` | `5` | 1–120 |
| `pomo_long_min` | `number?` | `15` | 1–120 |
| `pet_clock` | `bool?` | `true` | 收起时显示时钟 |
| `pet_ai_usage` | `bool?` | `false` | 收起时显示今日 token |
| `mcp_servers` | `McpServerConfig[]` | `[]` | `id`、`name`、`transport`、`command`、`args`、`env`、`url`、`enabled` |
| `monitor_bar_open` | `bool?` | `false` | 悬浮条是否显示 |
| `monitor_bar_click_through` | `bool?` | `false` | 鼠标穿透 |
| `monitor_bar_cpu` / `monitor_bar_mem` / `monitor_bar_net` | `bool?` | `true` | 分区开关 |
| `monitor_menubar_open` | `bool?` | `false` | macOS 菜单栏监控文字 |
| `monitor_menubar_show_icon` | `bool?` | `true` | 菜单栏监控期间保留托盘图标 |
| `monitor_refresh_ms` | `number?` | 前端归一 | 1000 / 2000 / 5000 |

`McpServerConfig.transport` 为 `stdio` 或 `http`；stdio 使用 `command` + `args` + `env`，http 使用 `url`。

## 7. 后台任务

| 任务 | 启动位置 | 周期 / 触发 | 产物 |
| --- | --- | --- | --- |
| 剪贴板轮询 | `lib.rs` setup → `clipboard_cmds::start_poll` | 每 1 秒 | 更新内存 + SQLite，`clipboard://changed` |
| AI 用量扫描 | `lib.rs` setup → `ai_usage_cmds::start_poll` | 每 5 分钟 | 增量写 `ai_usage_event`，有增量时发事件 |
| MCP 自动连接 | `lib.rs` setup → `mcp::connect_enabled_at_startup` | 启动一次 | 连接 `enabled` 的服务器，失败只记日志 |
| gRPC 通道 | 前端 `channel_start` | 长连接 + 心跳 / 重连 | Hello、任务、推送 |
| 图库扫描 / 去重 | `images_scan_start` / `images_dedup_start` | 手动 / 打开窗口 | 索引入库、缩略图、事件进度 |
| 文档扫描 | `docs_scan_start` | 手动 | 索引入库、事件进度 |
| 磁盘扫描 / 删除 | `disk_*_scan` / `disk_*_delete` | 手动 | 本地 SSE 进度 |
| 托盘重建 | `i18n://locale-changed` / `app://name-changed` | 事件 | 更新菜单文案与工具提示 |

## 8. 外部协议与端点

| 协议 / 服务 | 代码位置 | 说明 |
| --- | --- | --- |
| 账户 API | `src/api/auth.ts` | `https://hub.htui.cc/api/pas/user`，图形码 / 滑块 / 登录注册 |
| 收藏同步 | `src/composables/useStashCloud.ts` | `/api/cms/favorite/{batch,pull,batch_delete}`，裸 token；LWW |
| 剪贴板同步 | `src/composables/useClipboardCloud.ts` | `/api/cms/clipboard/{batch,pull}`，裸 token；仅文本 |
| 实时通道 | `src-tauri/src/channel.rs` | gRPC `/channel.v1.ChannelService/Open`，双向流 |
| MCP | `src-tauri/src/mcp.rs` | `rmcp` 0.8：stdio 子进程 / Streamable HTTP |
| 自动更新 | `tauri.conf.json` + `src/composables/useUpdater.ts` | `tauri-plugin-updater`，minisign 签名校验 |
| 发布上传 | `.github/workflows/hub-upload.yml` | `POST /api/core/release/upload`，multipart |
| 仓库数据刷新 | `useStashCloud.ts` | GitHub / Gitee 公开 API 更新 stars、forks、languages |
| 本地 SSE | `src-tauri/src/tauri_cmds/cleanup_cmds.rs` | `127.0.0.1:<random>/cleanup/events/<job>` |

安全约定：

- 云同步使用 `Authorization: <token>`（不带 `Bearer`），token 只存本地 `settings.json`。
- `http_request` 绕过 WebView CORS 但保留用户显式发起的语义；不要把它当作后台静默上报通道。
- `img://` 只接受 id，不直接拼路径；缩略图与原图都必须先查 DB。
- 更新器只信任配置中的公钥；私钥只存在于 CI secret 与本地发布环境。

剪贴板云同步的服务端字段、分页与墓碑语义见 [clipboard-sync-api.md](clipboard-sync-api.md)。
