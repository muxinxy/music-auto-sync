# Development Guide

> [中文](development.zh-CN.md) · [English](development.md)

## Architecture

Music Auto Sync is a Windows desktop application built with Tauri 2. The React frontend handles the interactive workflow, while Rust owns local storage, filesystem changes, background synchronization, audio conversion, and API access.

```text
src/                         React + TypeScript application
  App.tsx                    Shell, navigation, sync event listeners
  pages/                     Login, playlists, sync, quarantine, settings
  api.ts                     Typed Tauri command bridge
src-tauri/src/
  commands.rs                Tauri command handlers exposed to React
  api/                       NeteaseCloudMusicApi Enhanced HTTP client
  core/                      Naming and playlist synchronization engine
  ncm/                       NCM decrypt/convert implementation
  tags/                      Audio metadata writing through lofty
  store/                     Portable data paths, JSON config, SQLite
  runtime/                   Tray menu and startup/scheduled sync
scripts/
  build-windows.cmd          Local build helper for D:\Dev\VSBuildTools
  package-portable.ps1       Packages the built exe with an empty data/ dir
```

The frontend invokes Rust commands through `@tauri-apps/api`. Rust emits `sync://state`, `sync://progress`, and `sync://report` events back to the frontend while a task is running.

The local data directory contains `config.json`, `library.db`, `cache/`, and `logs/`. The resolver uses this order:

1. `--data-dir=<path>` or `--data-dir <path>`.
2. A path in `portable.ini` next to the executable.
3. A `data/` directory next to the executable.
4. The Windows local application data directory.

## Prerequisites

The supported build target is **Windows x64 MSVC**: `x86_64-pc-windows-msvc`.

Install the following before local development:

- [mise](https://mise.jdx.dev/) for the Node and Rust versions declared in `mise.toml`.
- Visual Studio 2022 Build Tools with the C++ build tools workload and Windows SDK. The local setup used by `scripts/build-windows.cmd` is installed at `D:\Dev\VSBuildTools`.
- WebView2 Runtime to run the desktop application. It is normally present on supported Windows installations, but a missing runtime prevents the Tauri window from opening.

Check the relevant toolchain from a Visual Studio Developer Command Prompt:

```powershell
cl
mise exec -- rustc -Vv
mise exec -- cargo -V
```

`rustc -Vv` must report `host: x86_64-pc-windows-msvc`. Do not switch this project to the GNU target: `.cargo/config.toml` and `mise.toml` intentionally pin the MSVC target.

## Setup

```powershell
mise install
mise exec -- npm ci
```

`npm ci` is required for repeatable dependency installation because it uses `package-lock.json`. Use `npm install` only when deliberately updating dependencies and the lockfile.

## Development

Run the Tauri development application:

```powershell
mise exec -- npm run tauri dev
```

Vite runs on `http://localhost:1420`. Its port is strict, so stop the process occupying port 1420 or change `vite.config.ts` before starting the app.

Run the frontend alone when only working on UI:

```powershell
mise exec -- npm run dev
```

## Verification

Build the frontend:

```powershell
mise exec -- npm run build
```

Type-check and run Rust unit tests:

```powershell
mise exec -- cargo check --manifest-path src-tauri/Cargo.toml
mise exec -- cargo test --manifest-path src-tauri/Cargo.toml
```

The Rust tests currently cover portable data layout and Windows-safe filename cleanup. Network, login, and actual music downloads are intentionally not run in automated tests because they require a valid account and API availability.

## Building Distribution Artifacts

Open a Visual Studio Developer Command Prompt, then run:

```powershell
mise exec -- npm run tauri build
powershell -ExecutionPolicy Bypass -File scripts/package-portable.ps1
```

For the local Build Tools installation at `D:\Dev\VSBuildTools`, this helper initializes MSVC and runs the Tauri build:

```powershell
scripts\build-windows.cmd
```

`build-windows.cmd` is machine-specific because it contains that fixed installation path. On another computer, use its Visual Studio Developer Command Prompt or call its own `VsDevCmd.bat` before the build commands.

Output paths:

```text
src-tauri/target/x86_64-pc-windows-msvc/release/music-auto-sync.exe
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Music Auto Sync_<version>_x64-setup.exe
release/music-auto-sync_x64_portable.zip
```

The portable archive contains a top-level `music-auto-sync_x64_portable/` directory with `Music Auto Sync.exe` and an empty `data/` directory. The empty directory selects portable mode on first launch.

## GitHub Actions and Releases

`.github/workflows/release.yml` uses `windows-latest`, the preinstalled Visual Studio toolchain, Node 24.19.0, and Rust stable MSVC.

- Pushes to `main` build the application and upload an Actions artifact containing both Windows files.
- Manual dispatches do the same without creating a release.
- Pushing a `v*` tag builds the files, uploads the workflow artifact, creates a GitHub Release using the matching `CHANGELOG.md` version section as its release notes, and attaches the NSIS installer and portable ZIP.

The workflow does not build from `scripts/build-windows.cmd`; that script is local-machine-specific. CI uses `npm ci`, runs the frontend build and Rust tests, runs `npm run tauri build`, and runs `scripts/package-portable.ps1`.

### Versioning a Release

Before creating a release, update the same version in all four files:

```text
CHANGELOG.md
package.json
src-tauri/Cargo.toml
src-tauri/tauri.conf.json
```

The changelog must contain a heading in this exact form:

```markdown
## [X.Y.Z] - YYYY-MM-DD
```

The release workflow extracts that section through the next `##` heading and uses it as the GitHub Release body. If the tag is `vX.Y.Z` and that section is missing, the publish job fails rather than creating a release with empty or incorrect notes.

Then commit the version update and push a matching tag:

```powershell
git tag vX.Y.Z
git push origin vX.Y.Z
```

The workflow does not rewrite versions from the tag. This prevents a tag from silently producing an application whose internal version differs from the release version.

Artifacts are not code-signed. Windows SmartScreen may warn users about an unsigned executable, particularly for new releases. Add code-signing secrets and a signing step before distributing broadly.

## Troubleshooting

### `link.exe` or MSVC is not found

Install Visual Studio Build Tools with the C++ workload and Windows SDK. Restart the terminal through a Visual Studio Developer Command Prompt, then confirm `cl` is available before running `npm run tauri build`.

### The Tauri application opens no window

Install or repair Microsoft Edge WebView2 Runtime, then relaunch the application.

### Port 1420 is already in use

Stop the process using the port or change `server.port` in `vite.config.ts`. Vite uses `strictPort: true`, so it will not select an alternative port automatically.

### QR login or playlist loading fails

Confirm the configured API base URL is reachable and compatible with [NeteaseCloudMusicApi Enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced). The QR requests include a timestamp to avoid cached status responses, and normal QR states `801` (waiting), `802` (scanned), `803` (success), and `800` (expired) are handled by the client.

An HTTP 403 means the configured API service or its CDN/WAF rejected the request. It is not a valid login state. Use a trusted compatible API instance, or configure an HTTP(S) proxy in Settings for API connectivity, for example `http://127.0.0.1:7897`. The proxy setting only controls the connection to the configured API service; it does not bypass music membership, copyright, regional, or other authorization restrictions.

Expired credentials require a new QR login. For an authorized QR session whose account state remains pending, use the login page's "打开登录日志目录" action and inspect `logs/login-diagnostics.jsonl`. It records only safe diagnostic fields such as endpoint, HTTP/API status, retry count, proxy configured state, and whether account/profile data exists. It never records cookies, QR keys/images, complete URLs, proxy addresses, or account data.

### A song has no download address

The API returns no URL for unavailable, VIP-only, copyright-restricted, or region-restricted music. The synchronizer records the failure and does not bypass access controls.
