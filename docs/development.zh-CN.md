# 开发文档

> [English](development.md) · [中文](development.zh-CN.md)

## 架构

Music Auto Sync 是基于 Tauri 2 的 Windows 桌面应用。React 前端负责交互流程，Rust 负责本地存储、文件系统操作、后台同步、音频转换与 API 访问。

```text
src/                         React + TypeScript 应用
  App.tsx                    外壳、导航、同步事件监听
  pages/                     登录、歌单、同步任务、隔离区、设置
  i18n.ts / locales/         国际化（中文 / English）
  api.ts                     带类型的 Tauri 命令桥接
  errors.ts                  UiMessage 错误码翻译助手
src-tauri/src/
  commands.rs                Tauri 命令处理
  api/                       NeteaseCloudMusicApi Enhanced HTTP 客户端
  error.rs                   UiMessage（可翻译的稳定错误码）
  core/                      命名（含歌手分隔符）与歌单同步引擎
  ncm/                       NCM 解密/转换实现
  tags/                      lofty 写音频元数据
  store/                     便携数据路径、JSON 配置、SQLite、登录诊断日志
  runtime/                   托盘菜单与启动/定时同步
scripts/
  build-windows.cmd          本机构建辅助（固定 D:\Dev\VSBuildTools）
  package-portable.ps1       将构建好的 exe 与空 data/ 打成便携包
```

前端通过 `@tauri-apps/api` 调用 Rust 命令；Rust 在任务运行时向 `sync://state`、`sync://progress`、`sync://report` 发送事件。进度事件的 phase/message 与同步结果错误均使用 UiMessage 稳定错误码，由前端按当前语言翻译，后端不保存具体语言文案（历史旧记录会原样回退展示）。

本地数据目录包含 `config.json`、`library.db`、`cache/` 与 `logs/`。数据目录优先级：

1. `--data-dir=<路径>` 或 `--data-dir <路径>`
2. exe 同级的 `portable.ini` 中记录的路径
3. exe 同级 `data/` 目录
4. Windows 本地应用数据目录

## 前置要求

支持的构建目标为 **Windows x64 MSVC**：`x86_64-pc-windows-msvc`。

- [mise](https://mise.jdx.dev/) 管理 `mise.toml` 中声明的 Node 与 Rust 版本。
- Visual Studio 2022 Build Tools（C++ 工作负载 + Windows SDK）。本机开发脚本 `scripts/build-windows.cmd` 使用安装在 `D:\Dev\VSBuildTools` 的构建工具。
- WebView2 Runtime：通常随受支持的 Windows 自带；缺失时应用窗口无法打开。

在 Visual Studio Developer Command Prompt 中验证工具链：

```powershell
cl
mise exec -- rustc -Vv
mise exec -- cargo -V
```

`rustc -Vv` 必须显示 `host: x86_64-pc-windows-msvc`。不要切换到 GNU 目标：`.cargo/config.toml` 与 `mise.toml` 有意固定 MSVC 目标。

## 初始化

```powershell
mise install
mise exec -- npm ci
```

必须使用 `npm ci` 以可复现方式安装依赖（基于 `package-lock.json`）。仅在有意升级依赖并更新锁文件时才用 `npm install`。

## 开发

启动开发应用：

```powershell
mise exec -- npm run tauri dev
```

Vite 运行在 `http://localhost:1420`，端口严格（`strictPort`）。若端口被占用，先停止占用进程或修改 `vite.config.ts`。

仅调试前端：

```powershell
mise exec -- npm run dev
```

## 验证

```powershell
mise exec -- npm run build
mise exec -- cargo check --manifest-path src-tauri/Cargo.toml
mise exec -- cargo test --manifest-path src-tauri/Cargo.toml
```

Rust 测试覆盖：便携数据目录布局、Windows 安全文件名清洗、多歌手分隔符、二维码状态码、UiMessage 序列化、登录诊断日志脱敏等。登录/下载相关自动化测试需要有效账号与 API 可用性，不纳入单元测试。

## 构建分发产物

在 Visual Studio Developer Command Prompt 中运行：

```powershell
mise exec -- npm run tauri build
powershell -ExecutionPolicy Bypass -File scripts/package-portable.ps1
```

本机专用脚本（Build Tools 位于 `D:\Dev\VSBuildTools`）：

```powershell
scripts\build-windows.cmd
```

其它机器请使用各自的 Visual Studio Developer Command Prompt 或 `VsDevCmd.bat`。

产物路径：

```text
src-tauri/target/x86_64-pc-windows-msvc/release/music-auto-sync.exe
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Music Auto Sync_<version>_x64-setup.exe
release/music-auto-sync_x64_portable.zip
```

便携包内是顶层 `music-auto-sync_x64_portable/` 目录，含 `Music Auto Sync.exe` 与空 `data/` 目录（空目录用于首次启动时选择便携模式）。

## GitHub Actions 与发布

`.github/workflows/release.yml` 使用 `windows-latest`、预装 Visual Studio 工具链、Node 24.19.0 与 Rust stable MSVC。

- 推送 `main`：构建并上传包含两个 Windows 产物的 Actions artifact。
- 手动触发：同上，不创建 Release。
- 推送 `v*` 标签：构建、上传 artifact、读取根目录 `CHANGELOG.md` 的 `## [X.Y.Z]` 小节作为 GitHub Release 正文，并附加 NSIS 与便携 ZIP。

CI 不使用 `scripts/build-windows.cmd`（本机路径专用）；CI 用 `npm ci`、前端构建、Rust 测试、`npm run tauri build` 与 `scripts/package-portable.ps1`。

### 发布一个版本

发布前同步更新四个文件中的版本：

```text
CHANGELOG.md（及 CHANGELOG.en.md）
package.json
src-tauri/Cargo.toml
src-tauri/tauri.conf.json
```

changelog 必须含精确标题：

```markdown
## [X.Y.Z] - YYYY-MM-DD
```

发布流程提取该标题到下一个 `##` 之间的内容作为 GitHub Release 正文；若缺失则发布失败，避免空或错误的更新说明。

提交并推送标签：

```powershell
git tag vX.Y.Z
git push origin vX.Y.Z
```

workflow 不会根据标签改写版本号，防止 tag 与实际内部版本不一致。产物未签名，Windows SmartScreen 可能对新版本弹出警告；如需正式分发请配置代码签名。

## 国际化

- 前端文案集中在 `src/locales/zh-CN.json` 与 `src/locales/en.json`，通过 i18next 在运行时即时切换，无需重启。
- 后端用户可见错误使用 `UiMessage`（稳定错误码 + 参数），由前端翻译；托盘菜单与窗口标题在切换语言时通过 `set_language` 命令即时重建。
- 修改“界面语言”会立即生效并保存；同步历史日志中的旧记录保留原语言展示。

## 故障排查

### `link.exe` 或 MSVC 未找到

安装 Visual Studio Build Tools（C++ 工作负载 + Windows SDK），通过 Developer Command Prompt 重开终端，确认 `cl` 可用后再执行 `npm run tauri build`。

### 应用无窗口

安装或修复 Microsoft Edge WebView2 Runtime 后重试。

### 端口 1420 被占用

停止占用进程，或修改 `vite.config.ts` 的 `server.port`（`strictPort: true` 不会自动换端口）。

### 二维码登录或歌单加载失败

确认 API 地址可达且兼容 [NeteaseCloudMusicApi Enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)。二维码请求带时间戳避免缓存；状态码 `801/802/803/800` 由客户端处理。

HTTP 403 表示 API 服务或其 CDN/WAF 拒绝了请求，可在设置中更换 API 地址或为 API 连通性配置 HTTP(S) 代理（例如 `http://127.0.0.1:7897`）。代理仅用于连接 API 服务，不绕过会员、版权、地区等授权限制。

若二维码已授权但登录状态未确认，可在登录页“打开登录日志目录”，查看 `logs/login-diagnostics.jsonl`。它只记录端点、HTTP/API 状态、重试次数等安全字段，不包含 cookie、二维码、完整 URL、代理地址或账号资料。

### 歌曲无下载地址

API 对不可用、VIP、版权或区域限制的歌曲不返回 URL；同步器会记录该失败，不绕过访问控制。

### 多歌手命名

文件名与 ID3 标签使用设置中的“歌手分隔符”，默认 `、`（例如 `歌手A、歌手B`）。修改分隔符只影响之后下载/重写的文件，不会自动重命名历史文件。