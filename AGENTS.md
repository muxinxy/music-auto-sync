# AGENTS.md

Windows 便携网易云音乐歌单同步器（Tauri 2 + Rust 后端 + React/TS 前端）。用户通过 GitHub Releases 发版，惯例英文提交信息。仓库根目录有一份大接口手册 `docs/api-enhanced.md`（网易云 API Enhanced 全部端点，改动登录/下载/歌单逻辑前先查它）。

## 目录与命令

- `src/` 前端 React+TS；`src-tauri/` Rust（Tauri）；`scripts/` 构建与只读调研脚本；`docs/` 文档。
- 构建/校验（Windows，MSVC target 由 `.cargo/config.toml` 与 `mise.toml` 固定）：
  - `mise exec -- npm run tauri build`（release；前端会先 `tsc && vite build`）
  - 本地快速校验：`cargo check --all-targets`（要零警告）、`cargo test`、`npx tsc --noEmit`、`npm run build`
  - i18n 键校验：`node scripts/check-i18n-keys.mjs`（zh/en 必须 parity，文案键增删后跑它）
  - 本地产物：`cmd /c scripts\build-windows.cmd`（NSIS）→ `pwsh -File scripts\package-portable.ps1`（便携 zip 到 `release/`）
- 工具链：mise 管 node/rust；**不要引入 GNU 工具链**（历史上 link 失败）。shell 沙箱对管道/引号有坑，复杂命令用 `pwsh -NoProfile -Command` 或临时 .mjs/.ps1 脚本；`node -e` 多行内联写文件有时被引号吞掉导致静默失败——改用脚本文件。`pwsh` 即 **Windows 上用 scoop 安装的 PowerShell 7**；不要用系统自带 Windows PowerShell 5（编码/GBK/BOM 问题会导致 JSON 解析失败），涉及写文件/解析优先 pwsh 或 node。

## 架构与数据流

- 前端通过 `src/api.ts` 的 `invoke` 调 Tauri 命令（`src-tauri/src/commands.rs` 注册于 `lib.rs` invoke_handler）。新增命令要：实现 → 注册 → `api.ts`/`types.ts` 加 wrapper。
- 后端分层：`api/mod.rs`（NeteaseApi HTTP client）→ `core/sync.rs`（同步引擎）→ `store/`（config.rs / database.rs / paths.rs / 日志）。`core/cloud.rs`（音乐云盘）、`ncm/`、`tags/`、`runtime/`（tray、scheduler）。
- 云盘：列表 `/user/cloud` 分页 200/页；上传走**客户端直传**（`/cloud/upload/token` → POST 文件到 uploadUrl 带 `x-nos-token`+`Content-MD5` 头 → `/cloud/upload/complete`），不受 API 代理服务器请求体限制，complete 会校验音频可解析性（非音频/部分 wav 会 400）。比对键 = 网易曲目 id（`simpleSong.id`/条目 `songId`，导入后网易会自动匹配官方曲目）。哨兵任务名 `"cloud"` 由前端 `src/taskName.ts` 翻译。
- 数据目录：`--data-dir` > exe 同级 `portable.ini` > exe 同级 `data/` > AppData。cookie/凭据只在本地 config.json；日志绝不写 cookie 明文。
- 配置存 `config.json`（`store/config.rs`，serde camelCase）；**新字段必须带 `#[serde(default)]` 或 default fn** 否则旧配置解析失败。
- SQLite `library.db`：`track_files` / `playlist_snapshots` / `quarantine` / `sync_logs` / `sync_runs` / `sync_changes` / `playlist_history` / `deleted_log`（建表幂等，加表在 `database.rs open()` 的 execute_batch 里）。

## 同步模型（关键，别改回去）

- 方向维度已删除。同步 = **下载模式（mirror/add_only/delete_only，作用于歌单→本地）** + 可选 **补录开关 upload_manual**（默认 false；仅"我创建的"歌单、只把本地多出的歌 add 进网易歌单，**绝不反向删歌/删歌单内歌曲**）。
- 歌曲不能从歌单删除/移出（无 UI）；"仅删除"模式 = 隔离本地多余文件到 `.quarantine`。删除都走隔离区或 deleted_log，可恢复。
- 同步引擎是并发下载：`core/sync.rs` 用 `tokio JoinSet` 滑动窗口 + 曲目边界检查 `pause_requested`/`cancel_requested`（AppState 里两个 Arc<AtomicBool>）。下载/备份也走同一机制。改并发逻辑别退回"全 spawn 再 join"（无法暂停）。
- `download_song_with_options` 下载到音乐根目录才登记 track_files（标记已同步）；自定义目录不算。

## 认证/API 约定（网易 API Enhanced）

- 会话必须用 **query/body `cookie` 参数**，HTTP Cookie 头不生效。登录/验证码/歌曲地址路由不加 `randomCNIP`（会破坏会员判定）。
- 登录：二维码 `/login/qr/*`；短信 `/captcha/sent` + `/login/cellphone`（需 timestamp cache-buster）。
- 错误处理：后端返回 `UiMessage {code, params}` JSON 字符串（`error.rs`），前端 `errors.ts translateUi` 按 `errors.<code>` 翻译。**code 与 locale 键必须一致**（历史坑：后端发 `sync_ok` snake、locale 只有 `syncOk` camel，导致中文界面显示英文原码——两边键名要完全一致）。**新增错误码一律 camelCase**（如 `cloudUploadFailed`，直译 locale 键）；存量 snake 码（`sync_busy` 等）是历史遗留、界面上会显示英文原码，勿模仿新代码。
- 音质预检用 `/song/detail` privilege；批量直链 `/song/url/v1`；本地文件匹配网易曲目用 `/search/match`。

## UI/产品约定

- 双语 i18n：`src/locales/zh-CN.json` + `en.json`，**每次新增文案两个文件同步加键**。
- 主题：Config `theme`(system/light/dark)；main.tsx ConfigProvider 按 `theme` + matchMedia 切 antd 深浅算法，设置改动后 `window.dispatchEvent(new Event("theme-changed"))` 让 Root 重读。
- 启动默认停"账号登录"页（登录后该页即账号信息+统计）。默认关闭启动自动同步/定时轮询。
- 托盘：左键开窗、右键菜单；暂停/继续/取消菜单项**只在有任务运行时出现**（歌单同步或云盘任务，二者可并行，pause/cancel 路由到在跑任务的标志）。任务运行态变化处（sync://state、cloud://state 发射点）要调 `runtime::tray::refresh(app)` 重建托盘。**永远不要直接调 `install`**（语言变化除外）：菜单回调跑在主线程，而 `run_on_main_thread` 在主线程上是就地同步执行的（tauri-runtime-wry 对主线程不排队），回调里同步销毁/重建托盘会和打开的菜单互锁死锁（应用未响应，0.8.1/0.8.2 两次踩坑）；`refresh` 内部先落工作线程再排队到主线程，任何上下文都安全。
- 云盘列表三级缓存：内存 TTL（ApiCache "user_cloud"）→ 磁盘缓存（`core/cloud.rs` 的 `load/save/clear_disk_cache`，cache 目录 `cloud-list.json`，带 cookie 指纹与 7 天有效期）→ 全量拉取。上传任务结束会清磁盘缓存；新增改动云盘内容的路径记得同步处理。
- 改动涉及歌单归属（创建 vs 收藏）用后端 `creator.userId` 与当前登录 uid 比对；收藏歌单不可写网易。

## 发版流程（用户经常做）

1. 升版本：`package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` 同步。
2. 双语 CHANGELOG（zh 供 CI 取 release notes）。
3. 提交（英文）+ `git tag vX.Y.Z` + push main + push tag → Actions 自动 release。
4. 本地产物：`scripts/build-windows.cmd` + `package-portable.ps1`。打包前确认无运行实例（release staging 的 exe 被占用会失败；`Get-Process | Where Path -like '*music-auto-sync*'` 找到后 Stop-Process）。
