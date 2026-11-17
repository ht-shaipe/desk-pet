# 开发指南

本文面向要改代码的人：如何把项目跑起来、代码放在哪、怎么扩展一个新页面 / 工具 / 窗口 / 命令 / 数据表，
以及 UI、i18n、调试方面的硬约定。运行时设计见 [architecture.md](architecture.md)，命令与表结构清单见
[backend.md](backend.md)。

## 1. 环境准备

| 依赖 | 要求 | 说明 |
| --- | --- | --- |
| Node.js | 20+ | 前端构建与 Tauri CLI；本文的 i18n 校验脚本需要 22.6+ |
| Rust | stable | `rustup` 安装；workspace 使用 edition 2024 |
| macOS | Xcode Command Line Tools | `xcode-select --install` |
| Windows | WebView2 + MSVC 构建工具 | 安装 Visual Studio Build Tools 的「使用 C++ 的桌面开发」 |
| 系统权限 | macOS 首次运行按提示授权 | 通知、自动化（深浅模式 / 静音）、完全磁盘访问（磁盘扫描） |

```bash
npm install
npm run tauri dev
```

`npm run tauri dev` 会通过 `beforeDevCommand` 启动 Vite（固定端口 **1820**，HMR **1821**），再编译并启动
Tauri 窗口。只跑 `npm run dev` 时页面能打开，但所有 `invoke` / 窗口 API 都不可用，仅适合调静态布局。

> Vite 端口是 `strictPort: true`。1820 被占用时会直接失败，先结束占用进程，不要改端口绕过——
> `tauri.conf.json` 的 `devUrl` 与开发文档都按 1820 约定。

## 2. 常用命令与验证

| 命令 | 作用 | 何时必须跑 |
| --- | --- | --- |
| `npm run dev` | 仅启动 Vite | 调纯前端样式 |
| `npm run tauri dev` | 完整桌面开发环境 | 联调 Tauri 命令 / 窗口 |
| `npm run build` | `vue-tsc --noEmit` + `vite build` | **所有前端改动** |
| `cargo check` | 检查主 crate | Rust 小改 |
| `cargo check --workspace` | 检查全部 crate | **所有 Rust 改动** |
| `cargo test --workspace` | 跑 workspace 单元 / 集成测试 | Rust 逻辑改动 |
| `make check` | `npm run build` + `cargo check --workspace` | 提交前 / 发布前 |
| `npm run tauri build` | 打当前平台安装包 | 验证打包、发版 |

约定：

- 前端没有独立的单元测试框架，**类型检查与构建就是最低门槛**；行为验证靠手动走查关键路径。
- Rust 测试主要覆盖 `deskpet-disk` / `deskpet-store` / `deskpet-os` 的纯逻辑，以及 `text_diff`、磁盘计划、
  AI 用量解析等命令内部函数。新增纯逻辑优先放到不依赖 Tauri 的 crate 或独立函数，便于补测试。
- 不要用 `cargo check` 代替 `cargo test --workspace`：测试会编译 `#[cfg(test)]` 模块与 fixture。

### 2.1 依赖变更

- 新增 npm / Cargo 依赖后，重新执行 `npm run build` 与 `cargo check --workspace`，并提交锁文件。
- 依赖变更还应更新 `THIRD-PARTY-NOTICES.md`。仓库当前 `npm run licenses:generate` 指向
  `scripts/gen-third-party-licenses.mjs`，但该脚本**尚未入库**；在补回脚本前，只能按
  `Cargo.lock` / `package-lock.json` 手工维护清单，不要把这条命令当作可用流程。

## 3. 代码边界

### 3.1 前端分层

| 目录 | 放什么 | 不要放什么 |
| --- | --- | --- |
| `src/views/` | 面板标签页，根节点用 `view-root` | 跨页面共享状态 |
| `src/components/` | 可复用组件、工具箱、精灵、布局、磁盘 treemap | 直接 `invoke` |
| `src/stores/` | Pinia 状态、初始化、乐观更新 | 大段 UI 模板 |
| `src/composables/` | 有副作用的组合逻辑（云同步、通道、更新、拖拽） | 纯函数 |
| `src/api/` | **唯一**的 `invoke` / 事件 / HTTP 封装入口 | 组件状态 |
| `src/lib/` | 纯逻辑与基础设施（`windowCtl`、`theme`、`stash`、`ai-tools`） | 依赖 Pinia |
| `src/types/` | 前后端共享类型 | 运行时代码 |
| `src/i18n/` | `zh-CN.ts` / `en.ts` | 硬编码用户可见文案 |

硬约定：

1. 组件里不写 `invoke("xxx")`，统一在 `src/api/<域>.ts` 暴露类型化函数。唯一例外是更新流程的
   `updater_open`：`useUpdater.ts` / `useChannel.ts` 直接调用窗口命令，避免与更新器封装形成依赖环。
2. Rust `serde` 数据字段保持 `snake_case`（前端类型也写 `snake_case`），不要在前端擅自转 camelCase；
   只有 Tauri 命令参数与显式标注 `#[serde(rename_all = "camelCase")]` 的 DTO 使用 camelCase。
3. 命令返回 `Result<T, String>`，前端拿到的是 reject 的字符串；面向用户的错误直接抛给
   `ElMessage.error(String(e))`，不要吞掉。
4. 新逻辑先判断能否下沉到 `deskpet-*` crate；`tauri_cmds/` 只做参数校验、任务编排、事件转发。

### 3.2 一次调用的完整链路

以「读取设置」为例：

```text
组件 / store
  └─ src/api/settings.ts        getSettings() → invoke<AppSettings>("get_settings")
       └─ src-tauri/src/lib.rs  generate_handler![... settings_cmds::get_settings ...]
            └─ tauri_cmds/settings_cmds.rs  #[tauri::command] pub fn get_settings(...)
                 └─ deskpet_store::settings::load(&dir)
```

新增命令时四处都要动：实现、注册、`api` 封装、调用方。漏注册的表现是运行时
`Command xxx not found`，而不是编译错误。

## 4. 扩展指南

### 4.1 新增面板标签页

1. **类型**：在 `src/stores/ui.ts` 的 `PanelView` 联合类型中加视图名。
2. **页面**：新建 `src/views/Xxx.vue`，模板根节点使用快捷类：

   ```vue
   <template>
     <div class="view-root">
       <div class="view-toolbar">
         <span class="toolbar-title">{{ t("xxx.title") }}</span>
       </div>
       <el-scrollbar class="view-scroll">
         <!-- 内容 -->
       </el-scrollbar>
     </div>
   </template>
   ```

   `view-root` = `flex flex-col h-full min-h-0`，`view-scroll` 负责限高与滚动条主题，别自己写
   `height: 100%` 或 scoped CSS。
3. **挂载**：在 `src/components/layout/PanelContainer.vue`：
   - `import Xxx from "@/views/Xxx.vue"`；
   - 在 `tabs` 数组加 `{ view: "xxx", label: t("tabs.xxx"), icon: SomeIcon }`；
   - 在内容区 `<Transition>` 中加一个 `v-else-if="ui.activeView === 'xxx'"` 分支。
4. **文案**：`src/i18n/{zh-CN,en}.ts` 同时加 `tabs.xxx` 与页面自己的键。
5. **深链（可选）**：`ui.openView("xxx")` 已能展开并切换；若需要 AI 工具或启动器跳转，复用该方法。
6. 跑 `npm run build`，手动检查 8 个标签切换、面板最小尺寸 300×420 下是否溢出。

> 标签内容是 `v-if` 按需挂载，不是 `v-show`。页面卸载会丢本地 `ref` 状态；需要跨切换保留的状态放进
> Pinia store。

### 4.2 新增工具箱工具

1. 新建 `src/components/toolbox/tools/XxxTool.vue`。它渲染在 `Toolbox.vue` 的详情区，根节点仍用
   `view-root`，滚动区自行管理；需要标题右侧补充信息时，可 `Teleport` 到 `#toolbox-toolbar-extra`。
2. 在 `src/components/toolbox/registry.ts` 的 `TOOLBOX_TOOLS` 追加：

   ```ts
   {
     id: "xxx",
     titleKey: "toolbox.tools.xxx.title",
     descKey: "toolbox.tools.xxx.desc",
     icon: SomeElementPlusIcon,
     component: defineAsyncComponent(() => import("./tools/XxxTool.vue")),
   }
   ```

3. 在 `zh-CN.ts` / `en.ts` 的 `toolbox.tools.xxx` 下加 `title`、`desc` 以及工具自身文案。
4. **独立窗口型工具**：若工具需要大画布（参考 `http` / `textdiff` / `docconv`），不要塞进面板详情，
   而是在 `src/views/Toolbox.vue` 的 `STANDALONE_WINDOW_IDS` 中登记，并在 `openStandaloneWindow()` 与
   `ui.openToolboxTool()` 中分流到对应的 `xxx_open` 命令；组件仍可保留在注册表里作为入口卡片。
5. 有 CPU 密集或文件 IO 的操作（压缩、解析、格式转换）放 Rust 命令并 `spawn_blocking`，不要在
   WebView 主线程做。

### 4.3 新增独立窗口

独立窗口复用 `index.html`，通过 `?view=` 选择根组件。**不要新增 HTML 入口**——多入口会改变
UnoCSS 与 Element Plus 的样式 chunk 顺序，导致主窗口样式被 EP 反超。

1. **前端根组件**：新建 `src/XxxApp.vue`，在 `onMounted` 里自行：
   - `getSettings()` 并应用 `setAppLocale` / `applyTheme` / `applyAccentColor`（独立窗口不继承主窗口状态）；
   - 调 `startWindowResize` 注册边缘缩放手柄（需要时给独立最小尺寸）；
   - 注册领域事件监听，再拉首屏数据。
2. **入口分流**：在 `src/main.ts` 的 `bootstrap()` 中加 `view === "xxx" ? (await import("./XxxApp.vue")).default : ...`。
3. **Rust 创建命令**：参考 `tauri_cmds/docs_cmds.rs::docs_open`：
   - `app.get_webview_window("xxx")` 存在则 `show` + `unminimize` + `set_focus`，返回 `false`；
   - 否则 `run_on_main_thread` 里用 `WebviewWindowBuilder` 创建 `WebviewUrl::App("index.html?view=xxx".into())`；
   - 无边框透明窗按平台加 `deskpet_os::window::round_window`（独立窗圆角 12）。
4. **注册与权限**：
   - 命令在 `src-tauri/src/lib.rs` 的 `generate_handler!` 注册；
   - 窗口 label 加入 `src-tauri/capabilities/default.json` 的 `windows` 数组，否则窗口内拿不到
     `core:window:*` 等权限；
   - 若用到新插件能力，在 `permissions` 中显式追加最小权限。
5. **前端封装**：在 `src/api/<域>.ts` 加 `xxxOpen()`，调用处不要直接 `invoke`。
6. 关闭窗口时记得释放 Rust 侧缓存（参考 `disk_analyze_close`）。

### 4.4 新增 Tauri 命令

1. 在 `src-tauri/src/tauri_cmds/<域>.rs` 实现：

   ```rust
   /// 一句话说明命令用途与副作用。
   #[tauri::command]
   pub async fn do_something(app: AppHandle, value: String) -> Result<Thing, String> {
       if value.trim().is_empty() {
           return Err("value 不能为空".into());
       }
       // CPU / 文件 IO：spawn_blocking；窗口操作：run_on_main_thread
       tauri::async_runtime::spawn_blocking(move || { /* ... */ })
           .await
           .map_err(|e| e.to_string())?
   }
   ```

2. 在 `src-tauri/src/lib.rs` 的 `generate_handler![...]` 中注册。
3. 在 `src/api/<域>.ts` 封装并补 `src/types` 类型。
4. 参数命名：Rust 用 `snake_case`，前端 `invoke` 的 key 用 `camelCase`（Tauri 会自动映射，例如
   `dataUrl` → `data_url`、`fromMs` → `from_ms`）。DTO 结构体若标注
   `#[serde(rename_all = "camelCase")]`，字段也按 camelCase 传。
5. 阻塞规则：
   - SQLite 操作持 `state.store.lock()`，不要在锁内做文件扫描 / 解码；
   - 窗口创建、`set_dock_visibility`、菜单栏绘制走 `app.run_on_main_thread`；
   - 磁盘扫描、图片解码、文档转换、日志解析走 `spawn_blocking` 或专用线程。
6. 长任务加取消标志（`AppState.image_tasks` / `doc_tasks` 的模式），并提供 `*_cancel` 命令。

### 4.5 新增数据表 / 字段 / 迁移

数据库在 `crates/deskpet-store/src/store.rs`，打开时执行 `DDL`、补列、迁移旧 JSON。

规则：

1. **新表**：在 `DDL` 末尾追加 `CREATE TABLE IF NOT EXISTS`，时间戳统一用 `INTEGER`（Unix 毫秒/秒）
   或与服务端契约一致的 `TEXT`（收藏用 `YYYY-MM-DD HH:MM:SS`）。
2. **给已有表加列**：`CREATE TABLE IF NOT EXISTS` 不会改旧表，必须在 `Store::open` 中追加
   `ALTER TABLE ... ADD COLUMN ... DEFAULT ...`，并像现有代码一样忽略「列已存在」错误以保持幂等。
3. **不要改已有列的类型或语义**；需要重构时新建表 / 新列，在启动迁移中回填。
4. 数据访问方法写在 `store.rs` 或同 crate 的模块（`gallery.rs` / `docs.rs` / `ai_usage.rs`），
   返回 `crate::Result<T>`；命令层只做 DTO 转换。
5. 查询注意分页与索引：图库 / 文档列表都有 `limit` / `offset`，新表按查询条件补索引。
6. 补迁移测试（参考 `store.rs` 的 `mod tests`），覆盖「旧数据存在」「重复启动」「空数据」三种情况。
7. 同步更新 [backend.md](backend.md) 的「SQLite 表」与 README 的「数据与本地文件」。

### 4.6 新增事件 / 后台任务

- 事件名统一 `域://动作`。Rust → 全部窗口用 `app.emit`，定向窗口用 `app.emit_to("label", ...)`。
- 高频事件要做节流（参考图库扫描每 20 条、缩略图每 4 张推一次），并在窗口关闭后停止发送。
- 后台线程的取消标志存入 `AppState`，命令侧置位；线程结束时移除标志，避免下次任务被误判为「进行中」。
- 启动型任务在 `lib.rs` 的 `setup` 中启动（剪贴板轮询、AI 用量每 5 分钟、MCP 自动连接）。

### 4.7 i18n

- 语言：`zh-CN`（默认）与 `en`；`fallbackLocale` 为 `en`。
- 两套文件的 **flat key 集合必须完全相等**，嵌套层级与键名一致，只允许值不同。
- 新增文案先加 `zh-CN.ts`，再补 `en.ts`；不要留空字符串占位（缺失时会回退英文或直接显示 key）。
- 托盘菜单在 Rust 侧按 `locale` 重建，所以语言切换必须 `emit("i18n://locale-changed", locale)`。

校验键位（Node 22.6+ 需带 `--experimental-strip-types`，Node 23.6+ 可省略；Node 20 需升级或用 `tsx`）：

```bash
node --experimental-strip-types --input-type=module -e "
const flat = (o, p = '') => Object.entries(o).flatMap(([k, v]) =>
  v && typeof v === 'object' ? flat(v, p + k + '.') : [p + k]);
const zh = (await import('./src/i18n/zh-CN.ts')).default;
const en = (await import('./src/i18n/en.ts')).default;
const a = new Set(flat(zh)), b = new Set(flat(en));
const onlyZh = [...a].filter((k) => !b.has(k));
const onlyEn = [...b].filter((k) => !a.has(k));
console.log({ zh: a.size, en: b.size, onlyZh, onlyEn });
process.exit(onlyZh.length || onlyEn.length ? 1 : 0);
"
```

## 5. UI 规范（UnoCSS）

新样式一律用 UnoCSS 工具类，**不新增 `<style scoped>`**。仅第三方库深度覆盖等极端情况例外。

### 5.1 主题 token

在 `uno.config.ts` 中注册：

- 颜色：`text-dp-primary/secondary/muted/tertiary/accent`、`bg-dp-panel-bg/card-bg/btn-bg/tabbar-bg`、
  `border-dp-card-border/divider/panel-border`、`bg-dp-tag-soft/danger-soft`；
- 阴影：`shadow-dp-card`、`shadow-dp-panel`、`shadow-dp-card-hover`；
- 动画：`animate-panel-in`、`animate-blink`、`animate-pip-pulse`、`animate-cleanup-slide` 等；
- 快捷类：`view-root`、`view-toolbar`、`toolbar-title`、`view-scroll`、`dp-scroll`、`dp-tip`、
  `dp-dialog`、`dp-form`、`rs-hint`。

颜色值来自 `src/styles/main.css` 的 `--dp-*` 变量，深浅主题各一套；不要在组件里写死 `#xxxxxx`
（主题色需要跟随用户设置变化）。

### 5.2 Element Plus 覆盖

EP 根元素自身的 class 直接写在组件上；内部元素用任意变体：

```html
<el-input class="[&_.el-input\_\_inner]:text-[11.5px]" />
```

模板里 `__` 必须写成 `\_\_`。状态标记用同元素变体（`[&.is-loading]:...`），祖先状态用
`[.msg-row:hover_&]:...`，相邻按钮用 `[&_.el-button+_.el-button]:ml-0.5`。

### 5.3 常见坑

- **渐变**用任意值：`bg-[linear-gradient(135deg,...)]`；不要用 `bg-gradient-to-*`，oklch 插值会偏色。
- **冲突属性**（背景、边框、文字色、字重）不要叠加两个 class 赌顺序，用互斥三元绑定：

  ```html
  :class="active ? 'bg-dp-accent text-white' : 'bg-dp-card-bg text-dp-muted'"
  ```

- 隐藏元素用 `hidden`；不要自造 `is-hidden` 再补 CSS。
- 语义类名只留给 JS 钩子或任意变体标记（`active`、`removed`、`failed`、`dragging`）。
- 减动效加 `motion-reduce:animate-none` / `motion-reduce:opacity-50`。
- 快捷类名不能以 `sm-` / `md-` / `lg-` / `xl-` / `2xl-` 开头，否则会被 UnoCSS 当成断点变体吞掉。
- 面板宽度只有 388（最小 300），字号/间距多用任意值（`text-[10.5px]`），不要套桌面端大间距。
- `el-tooltip` 统一 `effect="light"` + `popper-class="dp-tip"`，面板内加 `:teleported="false"`，避免收起时
  气泡锚点归零后闪现在精灵位置。

### 5.4 CSS 加载顺序（关键）

`src/main.ts` 中**四个样式导入**的相对顺序必须是：

```ts
import "element-plus/dist/index.css";
import "element-plus/theme-chalk/dark/css-vars.css";
import "virtual:uno.css";
import "./styles/main.css";
```

UnoCSS 与 EP 的规则常为同优先级，uno 必须后加载才能在冲突时胜出。

`src/notify-main.ts` **禁止导入 `virtual:uno.css`**。它会把 uno 拆成共享 chunk，并在 `index.html` 中先于
EP 的 `main-*.css` 加载，导致主窗口工具类被 EP 反超。提醒浮层自己写少量内联/独立样式即可。

> 验证构建产物时注意 minifier 会改写选择器：`\_` 归一化为 `_`、`rgba` 转 hex、keyframes 内
> `translateX(340%)` 变 `translate(340%)`。grep 产物要用宽松模式。

## 6. 调试与排错

### 6.1 日志

- **前端日志落盘**：`debugLog(scope, message)`（`src/api/system.ts`）写入应用数据目录的 `debug.log`，
  行格式 `[UTC 时间] [scope] message`，超过 512 KB 时保留尾部一半。
- **Rust 日志**：`eprintln!` 输出到 `npm run tauri dev` 的终端；托盘、通道、MCP、扫描失败都会打标记。
- **DevTools**：调试构建可在 WebView 打开开发者工具（macOS 通常 `Cmd+Option+I`，Windows 通常 `F12`，
  以实际构建为准）。注意右键菜单已被全局禁用，不能靠右键打开。
- **数据目录**：macOS `~/Library/Application Support/com.deskpet.desktop/`，Windows
  `%APPDATA%\com.deskpet.desktop\`；`settings.json` 损坏会被当作默认值，`deskpet.db` 可用
  `sqlite3` 直接查看。

### 6.2 常见现象

| 现象 | 排查方向 |
| --- | --- |
| 启动后精灵一直不显示 | `App.vue` 的首帧 `show()` 是否被异常打断；`settings.json` 里 `pet_x/pet_y` 是否在屏幕外（删除这两项） |
| 面板能开但点击输入无反应 | macOS accessory 模式下未取焦点；检查 `onBodyPointerdown` / `expandWindow` 的 `setFocus` |
| 命令报 `not found` | 忘记在 `lib.rs` 的 `generate_handler!` 注册，或命令名与 `api` 封装不一致 |
| 独立窗口空白 | `main.ts` 未加 `?view=` 分支；或窗口 label 不在 `capabilities/default.json` 的 `windows` 里 |
| 样式在打包后错乱 | 新增了 HTML 入口或调整了 CSS 导入顺序；检查是否违反 5.4 |
| 独立窗口不跟随主题 | 根组件没在 `onMounted` 调 `setAppLocale` / `applyTheme` / `applyAccentColor` |
| 扫描进度不更新 | 事件监听注册晚于任务启动；或窗口 label 写错导致 `emit_to` 丢包 |
| 更新检查失败 | 本地未配置签名私钥、`latest.json` 不可达，或版本号未同步（见 [release.md](release.md)） |
| 图库图片 404 | `img://` 协议按 id 反查失败（条目已删/缩略图缺失），或 Windows 下 `http://img.localhost` 代理未生效 |

### 6.3 前端调试技巧

- 打开页面时先看 Pinia：配置初始化失败会连带主题、窗口位置、云同步全部异常。
- Tauri `invoke` 的 reject 是字符串，不是 `Error`；`String(e)` 才能拿到可读信息。
- 云同步问题看 `debug.log` 的 `[stash-cloud]` / `[clipboard-cloud]` scope。
- 窗口几何问题优先看 `src/lib/windowCtl.ts` 的注释，里面记录了 macOS 的多个坑（拖拽阈值、
  `startResizeDragging` 未实现、程序化 resize 不触发 `onMoved`）。

## 7. 提交前检查清单

- [ ] `npm run build` 通过（含 `vue-tsc`）。
- [ ] `cargo check --workspace` 通过；改 Rust 逻辑时 `cargo test --workspace` 通过。
- [ ] 新增/修改文案在 `zh-CN.ts` 与 `en.ts` 中键位一致（跑 4.7 的脚本）。
- [ ] 面板在 300×420 与 388×588 下都不溢出；独立窗口在最小尺寸下可操作。
- [ ] 新 Tauri 命令已注册、已封装到 `src/api`、权限已声明。
- [ ] 数据库改动幂等，重复启动不报错、不丢数据。
- [ ] 依赖变更已更新锁文件；`THIRD-PARTY-NOTICES.md` 按当前可用方式同步。
- [ ] 文档同步：命令/事件/表结构 → [backend.md](backend.md)，窗口/流程 → [architecture.md](architecture.md)，
      用户可见行为 → [features.md](features.md)。
