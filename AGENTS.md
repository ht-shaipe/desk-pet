# Desk Pet 开发约定

桌面宠物应用：Tauri 2 + Vue 3 (`<script setup>` + TS) + Element Plus 2.13 + UnoCSS；Rust workspace。

## 常用命令

```bash
npm run dev                 # Vite dev（端口 1820，HMR 1821）
npm run build               # 前端构建（含 vue-tsc 类型检查）
cargo check                 # Rust 静态检查
cargo test --workspace      # Rust 全部测试
```

- 前端改动以 `npm run build` 通过为准；Rust 改动须过 `cargo check` 与相关 `cargo test`。
- i18n：`src/i18n/zh-CN.ts` 与 `en.ts` 键位必须完全对齐（flat 后集合相等），改动后重新比对。

## 样式：统一 UnoCSS，不写 scoped CSS

新样式一律用 UnoCSS 工具类；除第三方库深度覆盖等极端情况外不再新增 `<style scoped>`。

### 主题 token（uno.config.ts）

- 颜色：`text-dp-primary/secondary/muted/tertiary`、`text-dp-accent`、`bg-dp-card-bg/panel-bg/btn-bg`、`border-dp-card-border/divider` 等，均映射 `--dp-*` 变量。
- 阴影：`shadow-dp-card`、`shadow-dp-panel`、`shadow-dp-card-hover`。
- 动画：在 `theme.animation`（durations/timingFns/counts/keyframes）中注册后使用 `animate-xxx`，如 `animate-blink`、`animate-cleanup-slide`。
- 快捷类：`view-root`、`view-toolbar`、`toolbar-title`、`view-scroll`（含 el-scrollbar thumb 样式）。

### 已验证的关键写法

- EP 内部元素：任意变体 `[&_.el-input\_\_inner]:text-[11.5px]`（模板里 `__` 写成 `\_\_`）；EP 根元素自身的 class 直接写在组件上，后代用 `[&_.el-xxx\_\_yyy]:`。
- 状态标记类 + 同元素变体：元素带 `is-loading` 等标记时用 `[&.is-loading]:before:hidden`；祖先标记用 `[.msg-row:hover_&]:opacity-55`。
- 兄弟覆盖：`[&_.el-button+_.el-button]:ml-0.5`。
- 减动效：`motion-reduce:animate-none motion-reduce:opacity-50` 等变体。
- 渐变一律任意值：`bg-[linear-gradient(135deg,...)]`；**不要用 `bg-gradient-to-*`**（oklch 插值会与设计稿有偏差）。
- 可能冲突的属性（bg/border/color/font-weight）不要叠加 class，用互斥三元绑定 `:class="cond ? '...' : '...'"`——uno 输出顺序不保证后者胜出。
- 隐藏元素用 `hidden`，不用自定义类。
- 语义类名仅保留给 JS 钩子或任意变体标记（`active`、`removed`、`failed`、`dragging` 等）。

### CSS 加载顺序（关键）

`src/main.ts` 中的导入顺序必须是 Element Plus css → `virtual:uno.css` → `main.css`：
uno 与 EP 的规则常为同优先级，uno 必须后加载才能在冲突时胜出。
**`src/notify-main.ts` 禁止导入 `virtual:uno.css`**——它会把 uno 拆成共享 chunk 并在 index.html 中先于 EP 的 main-*.css 加载，导致主窗口工具类被 EP 反超。

### 产物验证注意事项

构建产物被 minifier 改写：选择器中 `\_` 归一化为 `_`、`rgba→hex`、keyframes 内 `translateX(340%)` 变 `translate(340%)`。grep 产物时用去转义/宽松模式。

## 结构

- `src/views/`：页面（Clipboard/Notes/Pomodoro/Chat/Account/QuickSettings/Settings/Toolbox…）。
- `src/components/toolbox/`：工具注册表 `registry.ts` + `tools/` 各工具组件。
- `src/api/`：Tauri invoke 封装；`src/types`：前后端共享类型。
- Rust：`src-tauri/`（主程序 + `tauri_cmds/` 命令）、`crates/deskpet-os`（系统能力）、`crates/deskpet-store`（SQLite 存储）。
- `src/i18n/{zh-CN,en}.ts`：UI 文案中英双语。

## 其他约定

- 面板窗口小（388×588，最小 300×420），macOS 为主平台；字号/间距多用任意值（`text-[10.5px]` 等）。
- 对话框/提示用 Element Plus（ElMessage/ElMessageBox/ElDialog）。
- Tauri 命令需在 `src-tauri/src/lib.rs` 注册；新权限在 `src-tauri/capabilities/` 声明。
