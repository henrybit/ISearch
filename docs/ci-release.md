# CI / Release 构建说明

本仓库通过 [`.github/workflows/build.yml`](../.github/workflows/build.yml) 在 GitHub Actions 上编译并打包：

| 平台 | Runner | 产物 |
|------|--------|------|
| macOS Apple Silicon | `macos-latest` | `.dmg` / `.app.zip`（`aarch64-apple-darwin`） |
| macOS Intel | `macos-latest` | `.dmg` / `.app.zip`（`x86_64-apple-darwin`） |
| Linux x86_64 | `ubuntu-22.04` | `.AppImage` / `.deb` |
| Windows x86_64 | `windows-latest` | NSIS `.exe` |

## 触发方式

- **Push** 到 `main` / `master`：构建并上传 Artifacts
- **Pull Request**：同上（fork PR 无法读取 secrets，macOS 为未签名包）
- **Tag** `v*`（例如 `v0.1.0`）：构建并创建 **Draft Release**
- **workflow_dispatch**：手动触发构建

发布示例：

```bash
git tag v0.1.0
git push origin v0.1.0
```

## 需要配置的 GitHub Secrets

路径：仓库 → **Settings → Secrets and variables → Actions → New repository secret**

### macOS 代码签名（必需，用于分发）

| Secret | 说明 |
|--------|------|
| `APPLE_CERTIFICATE` | Developer ID Application 证书导出的 `.p12`，再做 **base64** |
| `APPLE_CERTIFICATE_PASSWORD` | 导出 `.p12` 时设置的密码 |
| `KEYCHAIN_PASSWORD` | CI 临时钥匙串密码（任意强密码；可省略，workflow 会自动生成） |

导出与编码：

```bash
# 在 Keychain Access 中导出「Developer ID Application: Your Name (TEAMID)」为 MyCert.p12
openssl base64 -A -in MyCert.p12 -out certificate-base64.txt
# 将 certificate-base64.txt 全部内容粘贴为 APPLE_CERTIFICATE
```

分发到 Mac App Store **之外** 请使用 **Developer ID Application** 证书，并开启 Hardened Runtime（已在 `tauri.conf.json` 中配置）。

### macOS 公证（Notarization，强烈建议）

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

未配置公证相关 secrets 时：仍会签名（若有证书），但跳过 notarization，用户首次打开可能提示无法验证开发者。

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
- **macOS**：Homebrew `clamav`
- **Windows**：vcpkg `clamav:x64-windows` + Npcap SDK + LLVM（bindgen）

首次 Windows 构建会较慢（编译 ClamAV），之后靠 cache 加速。

## 产物位置

- Actions 页面 → 对应 run → **Artifacts**
- Tag 构建：Draft Release 附件（需在 GitHub Releases 页面手动 Publish）
