# Music Auto Sync

面向 Windows 的便携网易云音乐歌单同步器，使用 Tauri 2、Rust 和 React 构建。

它通过兼容 [NeteaseCloudMusicApi Enhanced](https://netease-api.muxinxy.com/) 的服务获取登录态、歌单和下载地址，将启用的歌单同步到本地音乐目录。

> 本工具仅用于同步你有权访问和保存的音乐内容。VIP、版权或区域限制导致 API 不返回下载地址时，应用会记录失败原因，不绕过服务端授权限制。

## 功能

- 网易云二维码登录，凭据仅保存在本地数据目录
- 手动同步、启动时同步、定时同步和系统托盘操作
- 缺失歌曲下载，多余歌曲先移入隔离区，可恢复或彻底删除
- 歌单文件夹、文件名、音质和并发数可配置
- 写入标题、歌手、专辑、音轨号和网易云 ID，可选 LRC、封面和 M3U8
- 自动扫描并转换 `.ncm` 文件，保留原文件并写入转换记录
- SQLite 本地索引、同步日志、单实例和取消任务
- 便携模式与可自定义数据目录，应用文件夹可整体迁移

## 下载

GitHub Actions 在推送 `v*` 标签后会自动创建 GitHub Release，并使用对应 `CHANGELOG.md` 版本小节作为更新说明；Release 会上传：

- `music-auto-sync_x64_portable.zip`：解压即用，无需安装
- `Music Auto Sync_<version>_x64-setup.exe`：NSIS 安装包

便携包内结构：

```text
music-auto-sync_x64_portable/
├─ Music Auto Sync.exe
└─ data/                    # 空目录会触发便携模式
```

## 使用

1. 运行 `Music Auto Sync.exe`。
2. 在“设置”中选择音乐根目录；默认 API 地址为 `https://netease-api.muxinxy.com`。
3. 在“账号登录”中扫码登录。若公共 API 返回 HTTP 403，可在“设置”中更换兼容 API 地址，或填写 HTTP(S) 代理地址后刷新二维码。
4. 在“歌单同步”中开启需要同步的歌单，点击“立即同步全部”或等待自动任务。
5. 被歌单移除的文件会进入 `<音乐根目录>/.quarantine/`，可在“隔离区”恢复或删除。

数据目录优先级如下：

1. 启动参数 `--data-dir=<目录>` 或 `--data-dir <目录>`
2. exe 同级 `portable.ini` 指向的目录
3. exe 同级 `data/` 目录
4. Windows AppData 目录

应用数据包括 `config.json`、`library.db`、`cache/` 和 `logs/`。设置页中的“更改并迁移”会迁移现有数据并更新 `portable.ini`。二维码已授权但状态未确认时，可在“账号登录”页选择“打开登录日志目录”，查看 `logs/login-diagnostics.jsonl`。该日志只记录请求状态、状态码和 profile/account 是否存在，不包含 cookie、二维码或账号资料。

## 本地构建

项目使用 mise 管理 Node 和 Rust 版本。Windows 构建要求 Visual Studio Build Tools 的 C++ 工作负载与 Windows SDK，当前支持目标为 `x86_64-pc-windows-msvc`。

```powershell
mise install
mise exec -- npm ci
mise exec -- npm run tauri dev
```

常用命令：

```powershell
mise exec -- npm run build
mise exec -- cargo test --manifest-path src-tauri/Cargo.toml
mise exec -- npm run tauri build
powershell -ExecutionPolicy Bypass -File scripts/package-portable.ps1
```

详细架构、调试、发布和 CI 说明见 [`docs/development.md`](docs/development.md)。
