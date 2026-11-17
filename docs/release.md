# 发布与自动更新

本文说明版本号来源、`Makefile` 发布流程、GitHub Actions 的构建 / 清单合并 / hub 上传、签名密钥与更新器
契约。日常开发与提交前检查见 [development.md](development.md)。

## 1. 版本号与产物

### 1.1 版本号来源

应用版本在 **3 个文件**中必须一致：

| 文件 | 字段 | 作用 |
| --- | --- | --- |
| `package.json` | `"version"` | 前端 / npm 生态版本，`Makefile` 的 bump 基准 |
| `src-tauri/Cargo.toml` | `version = "..."` | Tauri 主 crate 版本 |
| `src-tauri/tauri.conf.json` | `"version"` | 安装包与更新器读取的版本 |

workspace 根 `Cargo.toml` 没有 `version` 字段，`crates/*` 的 crate 版本与应用版本无关，均不参与同步。
`Cargo.lock` 会在版本变更后的 `cargo check --workspace` 中自动更新，发布提交必须带上它。

发布 tag 统一为 `v<semver>`，如 `v0.1.32`。CI 的 `resolve` job 会去掉可能存在的 `v` 前缀与空白，
校验 `^[0-9]+\.[0-9]+\.[0-9]+`；非法版本直接失败。

### 1.2 产物

| 平台 | 安装包 | 更新器产物 | 说明 |
| --- | --- | --- | --- |
| macOS arm64 | `.dmg` | `.app.tar.gz` + `.sig` | GitHub 资产名带 `-aarch64` 后缀，避免与 x64 覆盖 |
| macOS x64 | `.dmg` | `.app.tar.gz` + `.sig` | 资产名带 `-x86_64` 后缀 |
| Windows x64 | `.msi`、NSIS `*-setup.exe` | `*-setup.exe.sig` | 更新器下载 `setup.exe`，`installMode = passive` |

`tauri.conf.json` 开启 `bundle.createUpdaterArtifacts = true`，所以构建必须提供签名私钥。

## 2. 本地发布（Makefile）

```bash
make current-version              # 当前版本 / 下一 patch / 最新标签 / 分支
make dry-run                      # 预览将更新的文件与步骤，不执行
make release                      # 自动 bump patch：提交改动 → 同步版本 → 打 tag → 推送
make release '修复xxx问题'         # 说明文字作为提交未提交改动的 commit message
make release VERSION=0.2.0        # 发布指定版本
make minor                        # bump minor 并发布
make major                        # bump major 并发布
```

常用环境变量：

| 变量 | 作用 |
| --- | --- |
| `SKIP_CHECK=1` | 跳过 `npm run build` + `cargo check --workspace`（不建议） |
| `NO_PUSH=1` | 只本地提交 + 打 tag，不推送 |
| `RELEASE_MSG` / 位置参数 | 提交未提交改动时的 commit message |

`make release` 的实际步骤：

1. 校验目标版本 ≠ 当前版本，且 `v<目标版本>` tag 不存在。
2. 终端确认 `y`。
3. 未设置 `SKIP_CHECK=1` 时执行 `make check`。
4. 有未提交改动则 `git add -A && git commit`，消息默认 `chore: sync changes before vX.Y.Z`。
5. `make sync-version VERSION=X.Y.Z`：用 `perl` 同时改 3 个文件。
6. `cargo check --workspace` 更新 `Cargo.lock`。
7. 提交 `package.json`、`src-tauri/Cargo.toml`、`Cargo.lock`、`src-tauri/tauri.conf.json`，消息
   `vX.Y.Z: bump version`。
8. 打 tag `vX.Y.Z`。
9. 非 `NO_PUSH=1` 时推送当前分支与 tag，触发 CI。

> `make release` 会提交工作区**所有**未提交改动。执行前先确认 `git status`，不要把它当成只改版本号的命令。

### 2.1 本地打包与签名

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat ~/.tauri/desk-pet.key)"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="..."   # 私钥无密码时留空
npm run tauri build
```

- 私钥文件本身是单行 base64，**不要**解码、换行或重新编码；CI 会校验格式，含空白会直接失败。
- 私钥丢失后无法再向存量版本推送更新，只能让用户手动安装新包。
- 只验证前端/打包流程时可 `npm run build`，但它不会产出可发布更新包。

### 2.2 其他 Makefile 目标

| 目标 | 作用 |
| --- | --- |
| `make check` | `npm run build` + `cargo check --workspace` |
| `make sync-version VERSION=x.y.z` | 只同步 3 个文件的版本号，不提交、不推送 |
| `make clean` | 删除 `dist` 并 `cargo clean` |
| `make git 'msg'` / `make git MSG='msg'` | `git add .` + commit + pull + push |

## 3. CI 发布流程

工作流：`.github/workflows/release.yml`。

### 3.1 触发

- 推送 `v*` tag（`make release` 的正常路径）。
- `workflow_dispatch` 手动触发，输入 `version`（纯 semver，填 `v0.2.0` 也会清洗）。

### 3.2 Job 依赖

```text
resolve
  └─ publish（matrix: macos-arm64 / macos-x64 / windows）
       ├─ merge-manifest
       │    └─ hub-upload（可复用工作流）
       └─ notify（成功 / 失败都回调，取消不回调）
```

`resolve` 是全流程唯一的版本来源，输出 `version` 与 `tag`。下游 job 必须显式 `needs: resolve`，
否则 `needs.resolve.outputs.*` 为空——这是 GitHub Actions 的限制，历史故障中曾导致 `TAG="v"`。

### 3.3 `publish`

每个 target 独立执行：

1. checkout、Node 20、Rust stable + target、Rust cache、`npm install`。
2. 把 `src-tauri/tauri.conf.json` 的 `version` 同步为 `resolve` 输出（不改仓库文件，只改 runner 工作区）。
3. 幂等创建 GitHub Release（最多 5 次重试，兼容并发 job 的 `already exists` 与 API 瞬时 404/5xx）。
4. 校验 `TAURI_SIGNING_PRIVATE_KEY`：
   - 非空、无空白、合法 base64；
   - 解码后以 `untrusted comment:` 开头，且第二行是 minisign 密钥数据；
   - 若私钥加密（scrypt），需要 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
5. `npx tauri build --target <target>`。
6. 收集并上传资产：
   - macOS：`.app.tar.gz` / `.app.tar.gz.sig`（空格归一化为 `.`，加架构后缀）、`.dmg`；
   - Windows：`.msi`、NSIS `.exe`、更新器用 `.tar.gz` / `.sig`。
7. 生成 `fragment-<platform>.json` 并上传到同一 Release，作为 `latest.json` 的分片。

`platform` 映射：

| target | 清单 key |
| --- | --- |
| `aarch64-apple-darwin` | `darwin-aarch64` |
| `x86_64-apple-darwin` | `darwin-x86_64` |
| `x86_64-pc-windows-msvc` | `windows-x86_64` |

分片格式：

```json
{
  "version": "0.1.32",
  "platforms": {
    "darwin-aarch64": {
      "signature": "<.sig 文件内容>",
      "url": "https://hub.htui.cc/apps/desk-pet/desk-pet_macos_aarch64.app.tar.gz"
    }
  }
}
```

清单里的 URL 指向 **hub 扁平地址，不带版本号**，与服务端 `build_saved_name` 的落盘名一致；
带版本号会 404（历史故障 v0.1.30）。

### 3.4 `merge-manifest`

1. 下载 Release 中所有 `fragment-*.json`（最多 5 次重试）。
2. 合并 `platforms`，生成 `latest.json`：

   ```json
   {
     "version": "0.1.32",
     "pub_date": "2026-11-17T12:00:00.000Z",
     "notes": "See the assets below to download and install this version.",
     "platforms": { "...": { "signature": "...", "url": "..." } }
   }
   ```

3. 上传 `latest.json`（`--clobber`），再删除 Release 中的 fragment 资产，避免用户下载到中间文件。

### 3.5 `hub-upload`

调用可复用工作流 `.github/workflows/hub-upload.yml`，把安装包与 `latest.json` 推送到 hub 发版接口。
`release.yml` 只传 `tag` 与 `HUB_UPLOAD_TOKEN`，不继承全量 secrets。

工作流可直接 `workflow_dispatch`，也可被其他仓库引用：

```yaml
hub-upload:
  uses: ht-shaipe/desk-pet/.github/workflows/hub-upload.yml@main
  permissions:
    contents: read
  with:
    tag: v0.2.0
    app-key: your-app-key        # 留空回退仓库名
  secrets:
    hub_token: ${{ secrets.HUB_UPLOAD_TOKEN }}
```

#### 上传契约

| 项 | 值 |
| --- | --- |
| 端点 | `POST https://hub.htui.cc/api/core/release/upload` |
| 鉴权 | 请求头 `X-Upload-Token`，与服务端 `release.upload_token` 严格相等 |
| 表单字段 | `file`（单文件 / 次）、`appKey`、`version`（兼容 `v` 前缀）、`platform`、`arch`（macOS 双芯片必传）、`changelog` |
| `platform` | `0` 通用 / `1` iOS / `2` Android / `3` Web / `4` Windows / `5` Linux / `6` macOS |
| 成功响应 | `{ "code": 200, "result": { "filePath", "fileMd5", ... } }`（兼容旧契约 `code: 0`，HTTP 2xx） |
| 幂等 | 同 `appKey + version + platform + arch` 重复上传为更新记录；文件按目标名覆盖 |
| 落盘 | 安装包扁平命名，不带版本号；`latest.json` 固定 `apps/{appKey}/latest.json` |

上传顺序：

- 组间并行、组内串行：
  - `macos-aarch64`：`.app.tar.gz` → `.dmg`；
  - `macos-x86_64`：`.app.tar.gz` → `.dmg`；
  - `windows`：`.msi` → `.exe`。
- 所有安装包完成后，逐个校验 `latest.json` 中的 URL：HTTP 必须 200，且 `content-type` 不能是
  `text/html`（SPA fallback 会返回 200 + HTML，必须识破）。
- 最后上传 `latest.json`（`platform=0`），并回读校验 `version` 与本次发布一致、落盘路径必须是
  `/apps/{appKey}/latest.json`。

`.sig` 不上传——签名文本已经嵌入 `latest.json`。Windows 更新器直接下载 `setup.exe`。

超时 90 分钟：跨境链路实测可低至 ~18 KB/s，单文件 10–15 分钟；串行 6 个文件会撞超时（v0.1.26/27/29
的故障原因）。组间并行 + `curl --speed-limit 1024 --speed-time 60` 熔断重试是兜底。

### 3.6 `notify`

发布结束后回调外部接口，便于运营 / 通知系统感知发版：

| Secret | 必填 | 说明 |
| --- | --- | --- |
| `CALLBACK_URL` | 否 | 未配置时跳过并打 warning |
| `CALLBACK_TOKEN` | 否 | 配置后作为 `Authorization: Bearer <token>` |

请求体字段：`event`（`release.completed`）、`repository`、`version`、`tag`、`ref`、`sha`、`success`、
`jobs`、`workflow`、`event_name`、`run_id`、`run_number`、`run_url`、`release_url`、`occurred_at`。
请求头含 `X-GitHub-Event: release.completed` 与 `Idempotency-Key: <run_id>`。回调失败只记 warning，
不会把已完成的发布标记为失败。

## 4. Secrets

| Secret | 使用者 | 说明 |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | `publish` | 签名私钥文件原始内容（单行 base64，无空白） |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | `publish` | 私钥密码；无密码时可不配 |
| `HUB_UPLOAD_TOKEN` | `hub-upload` | hub 发版接口令牌 |
| `CALLBACK_URL` | `notify` | 发布回调地址 |
| `CALLBACK_TOKEN` | `notify` | 回调 Bearer token |
| `GITHUB_TOKEN` | 全部 job | GitHub 内置；创建 Release、上传资产、读 notes |

配置示例：

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY < ~/.tauri/desk-pet.key
gh secret set HUB_UPLOAD_TOKEN
gh secret set CALLBACK_URL
gh secret set CALLBACK_TOKEN
```

## 5. 自动更新

`src-tauri/tauri.conf.json`：

```json
{
  "plugins": {
    "updater": {
      "endpoints": [
        "https://hub.htui.cc/apps/desk-pet/latest.json",
        "https://github.com/ht-shaipe/desk-pet/releases/latest/download/latest.json"
      ],
      "pubkey": "dW50cnVzdGVkIGNvbW1lbnQ6...",
      "windows": { "installMode": "passive" }
    }
  }
}
```

行为：

- 启动后静默检查一次；之后每 6 小时周期检查（`useUpdater.ts`）。
- 折叠态发现新版本：打开 `updater` 独立小窗；可忽略该版本，忽略记录在本地。
- 展开态发现新版本：跳到「设置 → 关于」。
- 下载前校验 `latest.json` 的签名（minisign），签名不匹配拒绝安装。
- Windows 使用 passive 模式，安装过程显示进度但不要求交互。

更新失败排查顺序：

1. `latest.json` 是否可访问、`version` 是否大于当前版本；
2. 对应平台的 `url` 是否可达、`content-type` 是否为文件（不是 HTML）；
3. `signature` 是否与产物字节匹配（签名与托管地址无关，换 CDN 不影响）；
4. 私钥 / 公钥是否成对，CI 是否用了错误密钥。

## 6. 发布检查清单

- [ ] `git status` 干净或确认 `make release` 要一并提交的改动。
- [ ] 当前分支正确（通常是 `main` / `master`），且已同步远程。
- [ ] `make check` 通过；Rust 逻辑改动已跑 `cargo test --workspace`。
- [ ] 3 个版本文件一致，目标 tag 不存在。
- [ ] 依赖有变更时，第三方许可清单已按当前可用方式更新。
- [ ] 发布后确认 GitHub Release 资产齐全：两套 macOS `.dmg` / `.app.tar.gz` / `.sig`、Windows `.msi` /
  NSIS `.exe`、`latest.json`。
- [ ] 确认 hub `/apps/desk-pet/latest.json` 的 `version` 与新版本一致，且所有平台 URL 可下载。
- [ ] 用旧版本客户端实际走一次「检查更新 → 下载 → 重启」，确认签名与安装流程。

## 7. 回滚与失败处理

- **不要复用已发布的 tag**：Tauri 更新器与 hub 都以版本号为幂等键，重发同版本会导致清单 / 产物错配。
- 发布内容有问题时，修复后 bump 一个 patch 重新发布；必要时在 GitHub Release 标注 `pre-release`，
  但 hub 清单仍以最新上传为准。
- `publish` 部分平台失败：修复后重新运行 workflow（手动输入同一版本）或重新推送新 tag。Release 创建与
  资产上传都有重试，幂等可重入。
- `merge-manifest` 失败：检查 fragment 是否齐全、`resolve` 输出是否为空、`latest.json` 上传权限。
- `hub-upload` 失败：
  - `text/html`：hub 静态路由未生效，需要服务端为文件存储目录配置 `/apps/` 静态映射；
  - `latest.json` 路径不是 `/apps/{appKey}/latest.json`：服务端未按原始名落盘；
  - URL 不可达：上传文件名与服务端扁平命名不一致，或清单指向了带版本号的旧地址；
  - token 缺失 / 401：检查 `HUB_UPLOAD_TOKEN` 是否配置在调用方仓库。
- `notify` 失败不影响发布结果；回调接口应使用 `Idempotency-Key` 去重，避免重跑产生重复通知。
