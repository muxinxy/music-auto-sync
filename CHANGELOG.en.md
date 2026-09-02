# Changelog

> [English](CHANGELOG.en.md) · [中文](CHANGELOG.md)

This file records user-facing releases following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

## [0.3.0] - 2026-09-02

### Added

- UI localization: Chinese / English, switchable instantly in Settings without restarting; the tray menu and window title switch too.
- Configurable artist separator (default `、`), used consistently in filenames and ID3 tags.
- Single-song downloads support custom save directory, filename template, quality, lyrics, and overwriting existing files.
- CLI mode `--cli`: `status`, `sync <id|all>`, `download <playlist> <track>`, JSON output plus `--output` file writing.
- Closing the window minimizes to the system tray by default; can be disabled in Settings.
- Automatic quality fallback when unavailable (e.g. lossless → high → higher → standard).

### Fixed

- Single-song downloads no longer require the music root to be set when a save directory is chosen.
- The download dialog filename hint is no longer truncated.
- Single-song downloads no longer fetch lyrics when the lyrics checkbox is unchecked.
- No more duplicate tray icons after switching languages.
- The per-playlist “overwrite existing files” switch moved into each playlist row for quick access.

## [0.2.0] - 2026-09-02

### Added

- Playlist details and song list with status; single-song downloads with save directory, filename template, quality, lyrics and overwrite options.
- Per-playlist “synced x/y” progress and last sync time/result.
- Per-playlist “overwrite existing files” switch for re-downloading.
- Sync detects and registers existing local files as synced, avoiding re-downloading or overwriting user files.
- Settings auto-save; choosing the music root saves immediately.
- NCM conversion can keep or delete the original `.ncm` file.

### Fixed

- Changing settings no longer signs you out (auto-save preserved credentials and playlist config).
- Session expiry (`code:301`) now keeps playlist page and login page states consistent.
- Songs downloaded to an “unnamed” folder; playlist name now falls back to `/playlist/detail`.
- “Never synced” no longer sticks after syncing.
- Settings music root picker button alignment.

### Changed

- Default filename template is `{歌手} - {标题}`.
- README and dev docs reference [NeteaseCloudMusicApi Enhanced](https://github.com/NeteaseCloudMusicApiEnhanced/api-enhanced) as the compatible service.

## [0.1.1] - 2026-09-01

### Fixed

- QR login state codes and caching, plus HTTP 403 diagnostics.
- Effective session cookie was not persisted right after QR authorization.
- Login state check now passes the cookie through the Enhanced API `cookie` parameter.
- Release builds no longer open a console window.
- Replaced the blank/black icon with a multi-size music sync icon (window, taskbar, tray).
- Music root selection now displays and saves correctly.

### Added

- HTTP(S) proxy setting for API connectivity.
- Redacted JSONL login diagnostics with a log directory opener.

## [0.1.0] - 2026-08-31

### Added

- Windows desktop app with NetEase QR login and playlist management.
- Manual, startup and scheduled sync; missing-song download and quarantine of removed files.
- Folder/filename templates, LRC, M3U8, basic audio metadata and NCM auto-conversion.
- Portable mode, custom data directory migration, sync logs, system tray and single instance.
- NSIS installer and portable ZIP build flow.