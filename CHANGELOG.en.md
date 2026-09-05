# Changelog

> [English](CHANGELOG.en.md) · [中文](CHANGELOG.md)

This file records user-facing releases following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

## [0.6.0] - 2026-09-05

### Added

- Download modes: each playlist (or globally) can choose mirror / add-only / delete-only, applied to the playlist→local download side (mirror by default).
- Optional push-back: when “add manually placed local songs to the NetEase playlist” is enabled (globally or per playlist), sync matches local audio files not in the NetEase playlist (preferring the `.netease.json` sidecar, otherwise `/search/match` with title/artist/album/duration/md5) and adds them to the playlist. It applies only to playlists you created and is add-only — it never deletes NetEase tracks (so deleting a local file can never cascade into deleting it from the playlist). Off by default.
- Full change history: every add/delete of each sync (downloads, quarantined local extras, tracks added to or removed from the playlist) is written to a change log; the sync page lists them and allows per-track restore.
- Delete log with restore: quarantined local files can be moved back to their original path; tracks removed from a NetEase playlist can be added back; every sync saves a playlist snapshot so you can roll a playlist back to any historical state (owned playlists only).
- Playlist grouping: the list can be filtered by All / Created by me / Favorited (based on `creator.userId`); each playlist can override its direction and mode.
- Account stats panel: avatar, NetEase level, VIP, following/followers, created/favorited playlist counts, liked count, plus local cumulative sync stats (runs/added/quarantined/NCM converted/failed and current local file count).
- Standalone NCM converter: open from Settings, pick .ncm files or a folder to batch-convert, choose to keep or delete sources, and optionally ignore existing conversion markers.
- Avatar shown in the top bar.

### Changed

- Manual, startup, scheduled and tray sync all follow each playlist's direction × mode configuration.

## [0.5.0] - 2026-09-05

### Added

- Pre-flight availability/quality check before downloading: song details (`/song/detail` privilege) are fetched in batch so the song table can show per-song downloadability and the best quality your account can get; no-right / grey / purchase / region-limited tracks are flagged before you download.
- Concurrent downloads: tracks are downloaded in parallel up to the configured concurrency (semaphore limited) with batch pre-fetched URLs; each failed track is retried automatically (configurable count, exponential backoff).
- Batch failures no longer abort the run: the download dialog collects failures and offers "retry failed only"; sync results can be expanded to show each failed track and the reason.
- SMS verification-code login as an alternative to QR code login.
- "Back up to a local directory": download all Liked songs or all Purchased songs into a chosen folder (not counted as synced for any playlist).
- New "Clean removed files" action per playlist: files on disk that are no longer in the playlist are moved to quarantine.
- Song rows show local-file state: synced / missing / size, plus "show in folder".
- Silent update check against GitHub Releases on startup; a banner appears when a new version exists.
- New settings: request User-Agent, enable pre-flight before download, retry count per track.

### Changed

- Song-detail preflight degrades gracefully when an API instance does not support it, without blocking sync.

## [0.4.0] - 2026-09-04

### Fixed

- Member songs downloaded as preview clips: all requests now carry the session through the `cookie` parameter (the channel Enhanced forwards to NetEase), so quality follows your account entitlement.
- QR login stuck at "state not confirmed": removed the duplicate `cookie` parameter on `/login/status` that broke server-side parsing.
- Error toasts showing raw JSON: the frontend now parses and translates backend error codes.
- 88VIP member songs returning a 705KB/45s preview: URL endpoints no longer get `randomCNIP`, keeping membership checks intact.
- Downloading a single song into a custom directory outside the music root no longer marks it as synced.

### Added

- Per-track download log `logs/track-downloads.jsonl` (downloaded/skipped/failed, bytes, quality, error).
- General log `logs/app.log.jsonl` covering sync start/end/failure and command errors; logs rotate and are pruned by size.
- Settings option to choose the download-URL source: auto (song/url/v1 first) or prefer the song/download/url family.
- The app opens on the account login page by default; signed-in users are routed to the playlists page.
- 60-second cache for the playlist list to avoid lag when switching pages; manual refresh or mutations force a reload.

### Changed

- "Use random China IP" now defaults to off to avoid breaking membership-based quality decisions.

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