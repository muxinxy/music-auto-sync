# Music Auto Sync

A portable NetEase Cloud Music playlist synchronizer for Windows, built with Tauri 2, Rust and React.

It talks to any service compatible with [NeteaseCloudMusicApi Enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced) to fetch your login state, playlists and download URLs, and keeps enabled playlists in sync with a local music directory.

> This tool only syncs music you are entitled to access and save. When the API returns no download URL because of VIP, copyright or regional restrictions, the app records the failure and does not bypass the server-side authorization.

> [中文文档](README.md)

## Features

- NetEase QR code or SMS verification-code login; credentials are stored only in the local data directory
- Manual, startup and scheduled sync, plus a system tray
- Missing songs are downloaded; removed songs are moved to quarantine where you can restore or delete them
- Pre-flight availability/quality check: the song table shows per-song downloadability and the best quality your account can get before you download
- Concurrent downloads with batch pre-fetched URLs; per-track automatic retry and “retry failed only” for batch downloads
- Configurable playlist folder/filename templates, quality, concurrency, retry count, download URL source, and an artist separator used consistently in filenames and ID3 tags
- One-click backup of all Liked songs or all Purchased songs to a local folder
- Writes title, artist, album, track number and NetEase ID; optional LRC, cover and M3U8
- Scans and converts `.ncm` files, keeping or deleting the source as configured
- Song rows show local-file state (synced / missing / size) with “show in folder”
- Silent update check on startup with an in-app banner
- SQLite local index, sync logs, single instance and sync cancellation
- Portable mode with a customizable data directory; the app folder can be moved as a whole
- UI is available in Chinese and English; switching language takes effect immediately

## Download

GitHub Actions creates a GitHub Release (from the matching `CHANGELOG.md` section) when you push a `v*` tag, uploading:

- `music-auto-sync_x64_portable.zip`: extract and run, no installation needed
- `Music Auto Sync_<version>_x64-setup.exe`: NSIS installer

The portable archive layout:

```text
music-auto-sync_x64_portable/
├─ Music Auto Sync.exe
└─ data/                    # an empty directory enables portable mode
```

## Usage

1. Run `Music Auto Sync.exe`.
2. In “Settings”, choose the music root directory and, if needed, your API address (defaults to a compatible service of [NeteaseCloudMusicApi Enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced)).
3. Sign in on the “Account Login” page with the QR code or an SMS verification code.
4. On the “Playlists” page, enable the playlists you want to sync, then click “Sync all” or wait for the auto task. Open a playlist to browse its songs (with availability/local-file status), download individual songs, clean files no longer in the playlist, or back up Liked/Purchased songs from the top bar.
5. Files removed from a playlist go to `<music root>/.quarantine/`; manage them in “Quarantine”.

The app data directory is resolved in this order:

1. `--data-dir=<path>` or `--data-dir <path>`
2. The path in `portable.ini` next to the executable
3. A `data/` directory next to the executable
4. The Windows app data directory

It contains `config.json`, `library.db`, `cache/` and `logs/`. “Change & migrate” on the Settings page moves existing data and updates `portable.ini`.

## Building Locally

Node and Rust versions are managed with mise. Windows builds require Visual Studio Build Tools with the C++ workload and the Windows SDK; the supported target is `x86_64-pc-windows-msvc`.

```powershell
mise install
mise exec -- npm ci
mise exec -- npm run tauri dev
```

Common commands:

```powershell
mise exec -- npm run build
mise exec -- cargo test --manifest-path src-tauri/Cargo.toml
mise exec -- npm run tauri build
powershell -ExecutionPolicy Bypass -File scripts/package-portable.ps1
```

See [`docs/development.md`](docs/development.md) for architecture, releases and troubleshooting (中文开发文档：[`docs/development.zh-CN.md`](docs/development.zh-CN.md)).