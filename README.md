# Desk Pet 桌面精灵

基于 Tauri 2 + Vue 3 + TypeScript + Element Plus 的跨平台桌面宠物。

## 功能特性

- **桌面漂浮精灵**：无边框透明窗口、始终置顶、不占任务栏、可见于所有工作区（macOS），支持自定义头像
- **自由拖拽**：按住精灵拖到屏幕任意位置，位置自动记忆、重启恢复、贴边自动钳制
- **单击展开面板**，再次单击精灵或右上角按钮收起
- **快速启动器**：应用 / 网址 / 命令 三类入口，按分类展示，支持增删改、自定义图标与颜色
- **便签**：随手记录、多色彩、输入防抖自动保存
- **番茄钟**：专注 / 短休 / 长休循环，到时系统通知提醒
- **剪贴板历史**：自动捕获复制内容（仅文本，最多 100 条，超长自动截断），支持搜索、URL 识别高亮、一键回贴、单条删除、清空
- **快捷操作**：深浅模式切换、静音、锁屏、睡眠、直达系统设置各子页
- **工具箱**：可扩展的小工具集合，内置二维码生成/解析
- **账户**：手机号注册 / 登录（远端验证码 + 滑块人机验证），token 本地保存
- **多语言**：中文 / 英文，跟随系统语言，设置页可切换
- **系统托盘**：显示/隐藏精灵、退出
- **全局快捷键**：`Command/Ctrl + Shift + P` 显示/隐藏
- **开机自启**：设置页开关
- **单实例**：重复启动时唤起已有实例

### 工具箱扩展

工具箱采用「注册表」架构（`src/components/toolbox/`）：首页卡片列表 + 详情页，工具组件按需懒加载。新增一个工具只需三步：

1. 在 `tools/` 下新建组件（根节点撑满高度，滚动区自行管理）；
2. 在 `registry.ts` 的 `TOOLBOX_TOOLS` 数组追加一项（id、i18n 键、图标、组件）；
3. 在 `src/i18n/zh-CN.ts` 与 `en.ts` 的 `toolbox.tools.<id>` 命名空间补充中英文案。

## 工程结构

```
desk-pet/
├── Cargo.toml              # workspace 根（统一依赖版本与构建配置）
├── crates/
│   ├── deskpet-store/      # 数据层：SQLite 持久化、设置读写、头像管理（不依赖 Tauri，可独立测试）
│   └── deskpet-os/         # 系统层：应用启动、快捷操作、剪贴板访问、文件落盘（不依赖 Tauri）
├── src-tauri/              # Tauri 壳：窗口/托盘/插件注册，命令为薄壳委托到两个 crate
└── src/                    # 前端：views/ 页面、components/ 组件、api/ 按域封装、stores/ Pinia
```

## 平台支持

| 功能 | macOS | Windows |
| --- | --- | --- |
| 漂浮 / 置顶 / 拖拽 / 位置记忆 | 支持 | 支持 |
| 启动器 / 便签 / 番茄钟 / 剪贴板 | 支持 | 支持 |
| 深浅模式切换、静音 | 支持（首次使用需在系统弹窗中授权自动化） | 暂不支持 |
| 锁屏 / 睡眠 / 系统设置跳转 | 支持 | 支持 |

> Linux（尤其 Wayland 下透明与置顶窗口）暂不在支持范围内。

## 开发

前置要求：Node.js 20+、Rust stable 工具链。

```bash
npm install
npm run tauri dev    # 开发调试
npm run tauri build  # 打包发布
```

## 发布与自动更新

发布由 GitHub Actions（`.github/workflows/release.yml`）驱动：推送 `v*` tag 或手动触发 workflow，
自动构建 macOS（arm64/x64）与 Windows 安装包，上传到 GitHub Release，并合并生成 `latest.json`
更新清单。应用内置 `tauri-plugin-updater`，启动时静默检查更新、每 6 小时周期检查，发现新版本
弹通知可直接下载安装；设置页「关于」也可手动检查。

首次使用前需配置：

1. 在 GitHub 仓库 **Settings → Secrets → Actions** 添加：
   - `TAURI_SIGNING_PRIVATE_KEY`：`~/.tauri/desk-pet.key` 的内容
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`：生成密钥时设置的密码（本项目为空，但仍建议添加并留空，
     避免 CI 中 CLI 等待交互输入）
   - `CALLBACK_URL`（可选）：发布结束后的回调接口地址，未配置则跳过回调
   - `CALLBACK_TOKEN`（可选）：回调请求的 Bearer Token
2. 确认 `src-tauri/tauri.conf.json` 中 `plugins.updater.endpoints` 指向正确的 `owner/repo`
3. 发布：`git tag v0.2.0 && git push origin v0.2.0`（或 Actions 页面手动触发并填版本号）
4. 回调：所有平台打包（`publish`）与 `latest.json` 合并（`merge-manifest`）结束后，`notify` job
   向 `CALLBACK_URL` 发送一次 `POST`（成功、失败都会发，workflow 手动取消除外）。请求体包含
   `event`、`version`、`tag`、`success`、`jobs`、`run_url`、`release_url` 等字段；接口超时或非 2xx
   只记录 warning，不会把已完成的发布标记为失败。接口侧建议按 `run_id` 做幂等处理。

> 签名公钥已写入 `tauri.conf.json`；私钥务必妥善保管，丢失将无法再发布增量更新。

## 技术要点

- 透明窗口依赖 macOS 私有 API（`tauri.conf.json` 的 `macOSPrivateApi` + Cargo 的
  `macos-private-api` feature），因此**不能上架 Mac App Store**，个人使用无影响
- 拖拽与点击判定：Pointer 事件 + 4px 位移阈值 + 原生 `startDragging`，避免点击误触为拖拽
- 展开/收起采用**单窗口编程式 resize**（72×72 ⇄ 388×588），无需多窗口同步
- 数据持久化：SQLite（`deskpet.db`，WAL 模式，rusqlite bundled）存储便签/剪贴板/启动器，
  `settings.json` 存储设置项，均位于 `~/Library/Application Support/com.deskpet.desktop/`
  （Windows 为 `%APPDATA%\com.deskpet.desktop\`）；首次运行自动从旧版 JSON 迁移并留 `.bak`
- 剪贴板：Rust 后台线程每秒轮询（arboard），去重后经 Tauri 事件推送给前端
- 登录注册走 `tauri-plugin-http` 直连远端（免 WebView CORS 限制），token 存于 settings.json
- 番茄钟剩余时间基于时间戳计算，规避 WebView 定时器节流漂移

## 已知限制

- 精灵窗口为矩形，其透明边角会遮挡下方桌面点击（透明区域点击穿透暂未实现）
- `alwaysOnTop` 无法悬浮在 macOS 原生全屏应用之上
- 剪贴板历史仅记录文本，不含图片
