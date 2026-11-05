# CI / Release 构建说明

本仓库只有一套 GitHub Actions：

| Workflow | 文件 | 用途 |
|----------|------|------|
| **Release** | [`.github/workflows/release.yml`](../.github/workflows/release.yml) | 推送任意 tag 后打包并发布 GitHub Release |

| 平台 | Runner | 产物 |
|------|--------|------|
| macOS Apple Silicon | `macos-latest` | `.dmg`（`aarch64-apple-darwin`） |
| macOS Intel | `macos-latest` | `.dmg`（`x86_64-apple-darwin`） |
| Linux x86_64 | `ubuntu-22.04` | `.AppImage` / `.deb` |
| Windows x86_64 | `windows-latest` | NSIS `.exe` |

## 触发方式

推送任意 tag 时自动全平台打包并发布 Release（Release 名称与附件挂到该 tag 上）。

发布示例（建议先把 `package.json` 与 `src-tauri/tauri.conf.json` 的 version 改成一致）：

```bash
git tag 0.1.0
# 或: git tag v0.1.0
git push origin 0.1.0
```

发版完成后，`notes` job 会把 Release 说明改成带下载链接的列表。

## 需要配置的 GitHub Secrets

路径：仓库 → **Settings → Secrets and variables → Actions → New repository secret**

### macOS 代码签名（必需）

| Secret | 说明 |
|--------|------|
| `APPLE_CERTIFICATE` | Developer ID Application 证书导出的 `.p12`，再做 **base64** |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 `.p12` 时设置的密码 |

导出与编码：

```bash
# 在 Keychain Access 中导出「Developer ID Application: Your Name (TEAMID)」为 MyCert.p12
openssl base64 -A -in MyCert.p12 -out certificate-base64.txt
# 将 certificate-base64.txt 全部内容粘贴为 APPLE_CERTIFICATE
```

分发到 Mac App Store **之外** 请使用 **Developer ID Application** 证书，并开启 Hardened Runtime（已在 `tauri.conf.json` 中配置）。

### macOS 公证（Notarization，必需）

任选 **一种** 鉴权方式。

#### 方式 A：App Store Connect API Key（推荐 CI）

| Secret | 说明 |
|--------|------|
| `APPLE_API_KEY` | Key ID（例如 `AB12CD34EF`） |
| `APPLE_API_ISSUER` | Issuer ID（UUID） |
| `APPLE_API_KEY_P8` | `AuthKey_xxx.p8` 文件内容（原始 PEM）或该文件的 base64 |
| `APPLE_TEAM_ID` | Team ID（10 位） |

在 [App Store Connect → Users and Access → Integrations → Team Keys](https://appstoreconnect.apple.com/access/integrations) 创建 API Key（建议 Developer 权限）。

#### 方式 B：Apple ID + App-Specific Password

| Secret | 说明 |
|--------|------|
| `APPLE_ID` | Apple ID 邮箱 |
| `APPLE_PASSWORD` | [App-Specific Password](https://appleid.apple.com/) |
| `APPLE_TEAM_ID` | Team ID |

未配置签名/公证相关 secrets 时，macOS job 会直接失败。

## 本地验证签名（可选）

```bash
# 签名信息
codesign -dv --verbose=4 /path/to/ISearch.app

# 公证票据（stapler）
spctl --assess --type execute -vv /path/to/ISearch.app
```

## 系统依赖说明

应用链接了 **ClamAV** 与 **libpcap**，CI 中会安装：

- **Linux**：`libclamav-dev`、`libpcap-dev`、WebKitGTK 等
- **macOS**：Homebrew `clamav` / `openssl@3`。`scripts/macos-vendor-dylibs.sh` 会把 libclamav 及其非系统依赖复制到 `src-tauri/macos-libs/`、改为 `@rpath` 引用，并生成 `src-tauri/tauri.macos-libs.conf.json`，打包时放进 `ISearch.app/Contents/Frameworks` 并一起签名，用户无需安装 Homebrew。本地 `npm run tauri` 会自动执行该脚本。x86_64 包在 Intel 机器 `macos-15-intel` 上原生构建（Homebrew 在 arm64 机器上只有 arm64 库；GitHub 的 Intel macOS 机器提供到 2027 年 8 月）。
- **Windows**：vcpkg `clamav:x64-windows` + Npcap SDK + LLVM（bindgen）

首次 Windows 构建会较慢（编译 ClamAV），之后靠 cache 加速。

## ClamAV 病毒库

病毒库（`.cvd`）与平台无关，不再提交到仓库（`clamav_db/` 已加入 `.gitignore`）。

- **打包**：`tauri.conf.json` 把仓库根目录的 `clamav_db/` 作为资源打进安装包。`beforeBuildCommand` 会先运行 `scripts/fetch-clamav-db.js`：病毒库不全时优先用本机的 `freshclam`，没有则直接从 database.clamav.net 下载 `main` / `daily` / `bytecode.cvd`；下载失败则打包失败。`tauri dev` 下同样会尝试下载，但失败只告警。
- **CI**：`clamav-db` job 在 Ubuntu 上用 `freshclam` 下载病毒库，按周缓存（官方会对 CI IP 限流），作为 artifact 分发给各平台 job；打包时脚本检测到已存在就跳过下载。
- **运行时查找顺序**：安装包资源目录 `clamav_db` → （仅 debug 构建）仓库根目录 `clamav_db` → `~/.isearch/clamav_db` → 系统 ClamAV 目录；都没有时自动下载到 `~/.isearch/clamav_db`。

## 产物位置

GitHub Releases 页面附件（含分平台下载链接）。
