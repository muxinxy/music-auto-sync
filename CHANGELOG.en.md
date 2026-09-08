# Changelog

> [English](CHANGELOG.en.md) · [中文](CHANGELOG.md)

This file records user-facing releases following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

## [0.8.8] - 2026-09-08

### Fixed

- Downloaded songs showed no cover art in Windows Explorer: the APIC frame description was written as UTF-16, which Explorer handles poorly; it is now written as Latin1 (matching official NetEase files) so covers display correctly.
- Newly written MP3 tags now use ID3v2.3 (matching official NetEase downloads); fresh downloads previously defaulted to v2.4, which Explorer also handles poorly for APIC.

## [0.8.7] - 2026-09-08

### Fixed

- Downloaded songs could end up without cover art: the cover embed previously ran as a background task concurrently with the "163 key" tag write, and the two writers racing on the same file could overwrite each other (missing covers). Cover embedding now runs strictly after the 163 key write. Already-downloaded files are not backfilled automatically — re-download them (or enable quality auto-upgrade) to add covers.

### Changed

- "Auto-upgrade quality" now works for any target tier, not just lossless: when a local file's actual bitrate (read from the file) is below the configured target, sync re-downloads a higher-quality copy (e.g. 128k→320k, 192k→320k, mp3→lossless). When the account tier or copyright limits mean NetEase cannot serve a higher tier (preflight available level below target), it does not repeatedly re-download the same file.

## [0.8.6] - 2026-09-07

### Added

- New "About" page (sidebar bottom): version, tech stack, data & privacy notes, plus links to the GitHub repository, releases and the API repo.
- New "Auto-upgrade to lossless" setting: when quality is lossless (hires/lossless) and an existing local file is lossy (mp3/m4a/etc.), sync re-downloads a lossless copy to replace it (also cleaning up the old .lrc and sidecar); lossy quality targets never trigger re-downloads.

## [0.8.5] - 2026-09-07

### Added

- The "Embed lyrics tags" setting now actually works: when enabled, downloaded songs get their lyrics written into the audio file's own tags (USLT frame for MP3, LYRICS field for FLAC/others), independent from — and stackable with — "Save a matching .lrc file". Embedding failures never block the download. Previously the option was toggleable in the UI but never read by the backend, so it had no effect.

### Fixed

- The "Write album cover" setting was also ineffective: covers were embedded unconditionally, so turning the option off still embedded them. Cover embedding now honors the switch.

## [0.8.4] - 2026-09-07

### Added

- Downloaded songs now embed album art (ID3 APIC / FLAC picture) so players and Windows Explorer show covers. Both playlist sync and single-song downloads (including custom directories) embed art; fetching is async and never blocks the download itself.

## [0.8.3] - 2026-09-07

### Fixed

- Tray Pause/Resume/Cancel still froze the app (the 0.8.2 fix was incomplete): per the tauri-runtime-wry source, `run_on_main_thread` executes inline when already on the main thread instead of queuing, so 0.8.2 still rebuilt the tray synchronously inside the menu callback. The rebuild now hops to a dedicated worker thread first and is then queued through the event loop, guaranteeing it runs only after the menu event handling has fully returned.

## [0.8.2] - 2026-09-07

### Fixed

- Clicking Pause/Resume/Cancel in the tray froze the app (not responding): the menu callback rebuilt the tray icon synchronously, deadlocking the main thread against the open menu's internal state. The rebuild is now queued to run after the menu event handler returns.
- Match preview statuses are more accurate: matched-but-unregistered files always showed "Pending rename/register" even when the file name already matched the current naming template (sync would register it directly without renaming). The status now distinguishes "Pending rename/register" from "Pending registration (name matches template)", using the same rule as the sync engine's rename decision.
- Pause/Resume actions in the UI now also refresh the tray menu labels instead of leaving them stale.

## [0.8.1] - 2026-09-06

### Added

- The cloud song list is now cached on disk: after restarting the app the cloud page opens instantly with the last known list and refreshes in place in the background; the cache is invalidated when the account changes and cleared after uploads.

### Fixed

- The tray menu now refreshes as tasks start and end: previously the tray was only rebuilt at startup and on language change, so Pause/Resume/Cancel were effectively unusable once a task started; cloud tasks are now supported too — previously the tray could only control playlist sync, and cloud uploads/downloads could not be paused or canceled from the tray.

### Changed

- The release workflow now verifies zh/en locale key parity; out-of-sync locale keys fail the build.

## [0.8.0] - 2026-09-06

### Added

- **Cloud disk**: a new "Cloud Disk" page in the sidebar listing your NetEase cloud-disk songs (count/used-space stats, search filter, refresh) with a 120-second cache.
- **Sync to cloud disk**: scans the local music root and uploads songs missing from the cloud disk. Uses client-side direct upload (token → object storage → complete), unaffected by API-proxy request-body limits, so FLAC and other large files work; files already on NetEase servers are instant-imported; jumps to Settings when the music root is not configured.
- **Manual upload**: pick audio files (multi-select) or an entire folder on the Cloud Disk page to upload; folders are scanned recursively for audio.
- **Multi-select download**: select cloud songs (selection preserved across pages) and download them to a folder of your choice (defaults to the music root; existing files are skipped).
- **Parallel tasks**: cloud tasks (upload/download) and playlist sync run independently and simultaneously, each with its own progress and pause/resume/cancel; the sidebar status reflects any running task.
- **Task details**: every sync-log entry can be opened to inspect that run — cloud tasks show per-song comparison (already in cloud / duplicate / unmatched) plus upload/download results; playlist tasks show downloads, quarantines, playlist push-backs and removals. Failed rows show the exact reason, deleted rows can be restored directly. Supports filtering by file name and status/action, tri-state column sorting (asc → desc → default), and live refresh while a task is running.
- The avatar/nickname in the header is clickable and returns to the account login page.

### Changed

- One sync task now keeps exactly **one** sync-log entry: "running" at start, updated in place to success/failure/canceled at the end, with the failure/cancel reason written into the log body; the log list refreshes promptly when tasks start and finish.
- The former "change records" and "restorable deletions" lists are merged into sync-log task details.
- Sync logs support date-time range filtering; search matches translated task names (searching "cloud" finds cloud tasks).
- Match preview no longer starts automatically: pick a playlist or press "Start matching" to run it.
- Settings page columns are top-aligned with the whitespace in the bottom-right corner.
- Error messages are now fully localized: backend error codes match translation keys, and HTTP/network errors no longer show raw English codes.

### Fixed

- A canceled playlist sync no longer leaves a stale "running" sync-log entry.
- Playlists not registered in sync settings no longer show `#<playlist id>` as the task name.
- The cloud-disk list cache previously never took effect (all pages were re-fetched on every visit).

## [0.7.2] - 2026-09-06

### Fixed

- The "Download" link in the new-version banner had a doubled `v` (`.../tag/vv0.7.1` could not be opened): the new version is now returned as a bare version number and both the label and the link add the `v` prefix exactly once.

## [0.7.1] - 2026-09-06

### Changed

- Removed the "Back up to a local directory" button from the playlist page (and its backend logic).
- NCM conversion now names the output file after the source .ncm file (instead of the embedded song name), so the converted file keeps the original name with a new extension.
- The main window now opens centered on the screen.
- The "Refresh stats" button on the account page now shows a spinner and a card overlay while refreshing, with clear success/failure feedback.
- The NeteaseCloudMusicApiEnhanced link in Settings → "NetEase API address" is now clickable and opens the GitHub repo in the default browser; the "Download" button in the new-version banner is fixed the same way.

### Fixed

- NCM conversion failure: files such as `李荣浩 - 年少有为.ncm` previously failed with "invalid NCM AES padding" — the metadata decryption used the wrong key and the audio stream cipher did not match NetEase's implementation. Both are now corrected to match the official ncmdump output.
- NCM converter modal: no longer shows a misleading 0% progress bar while running (replaced with a spinner); failure details are no longer truncated to the first 5.
- The "current task" card no longer keeps showing a stale (0%) progress after a sync/conversion ends.

## [0.7.0] - 2026-09-05

### Added

- Local match preview: a “Match preview” entry on the playlists page shows how local audio files map to NetEase tracks per playlist. It first parses the official `163 key(Don't modify)` comment (AES-decrypts the embedded NetEase song id for exact matching), then falls back to ID3 tag title + artists. Match sources are labelled sidecar / 163 key / netease-id / tag / unresolved.
- Newly downloaded songs get an official-format 163 key comment (enc=0 Latin1 + lang=XXX, byte-identical to files downloaded by the NetEase client, so Windows properties shows it); the NetEase id is no longer written as plain text.
- Language / theme quick switchers in the top bar (available on every page, applied and saved immediately).
- Local-file reuse on sync: when a playlist folder already holds the same song (recognized via sidecar / 163 key / tags), it is registered as synced and renamed to the current template instead of being re-downloaded.

### Changed

- Data freshness & performance: NetEase metadata (playlist list/tracks/lyrics/account profile) now has an in-process TTL cache so repeated UI reads don't re-request; the sync engine always uses fresh data and is never cache-stale. Refresh buttons and match preview can force a cache-bypassing fetch.
- Sync concurrency: workers share the whole playlist/config instead of deep-cloning large objects per track.
- Dark theme softened: backgrounds are no longer pure black and borders have lower contrast; login/settings content now fills the window responsively; settings items moved to a two-column layout.
- Theme switching applies immediately (config is persisted before the change event is dispatched — fixed an ordering bug).

### Fixed

- Playlist-list cache leaking across accounts (old account's playlists shown after logout/switch) — cache is now keyed by account.
- Download progress no longer re-renders the whole UI tree (progress has its own subscription); the idle 1-second poll is stopped when nothing is running.
- Match-preview dialog hidden behind the song-list drawer (z-index).

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