import { invoke } from "@tauri-apps/api/core";
import type {
  AccountStats,
  AppInfo,
  BatchItemResult,
  Config,
  DeletedLogEntry,
  LocalMatchPreview,
  LocalStats,
  LoginStatus,
  NcmConvertReport,
  PlaylistHistoryEntry,
  PlaylistInfo,
  PlaylistSongsResult,
  QuarantineItem,
  QrCheckResult,
  SingleDownloadOptions,
  SyncChangeEntry,
  SyncReport,
  TrackAvailability,
} from "./types";

export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getConfig: () => invoke<Config>("get_config"),
  saveConfig: (config: Config) => invoke<Config>("save_config", { config }),
  setDataDir: (dir: string, migrate: boolean) =>
    invoke<AppInfo>("set_data_dir", { dir, migrate }),

  getLoginQr: () => invoke<{ key: string; qrImg: string }>("get_login_qr"),
  checkLoginQr: (key: string) => invoke<QrCheckResult>("check_login_qr", { key }),
  getLoginStatus: (verifyAttempt?: number, retryLimit?: number) =>
    invoke<LoginStatus>("get_login_status", { verifyAttempt, retryLimit }),
  openLoginLogDirectory: () => invoke<void>("open_login_log_directory"),
  setLanguage: (language: string) => invoke<void>("set_language", { language }),
  logout: () => invoke<void>("logout"),
  sendLoginCaptcha: (phone: string) => invoke<void>("send_login_captcha", { phone }),
  loginWithCaptcha: (phone: string, captcha: string) =>
    invoke<LoginStatus>("login_with_captcha", { phone, captcha }),

  listPlaylists: (force?: boolean) =>
    invoke<PlaylistInfo[]>("list_playlists", { force: force ?? false }),
  getPlaylistSongs: (id: number, force?: boolean) =>
    invoke<PlaylistSongsResult>("get_playlist_songs", { id, force: force ?? false }),
  downloadSongWithOptions: (
    playlistId: number,
    trackId: number,
    options: SingleDownloadOptions
  ) =>
    invoke<string>("download_song_with_options", { playlistId, trackId, options }),
  setPlaylistEnabled: (id: number, enabled: boolean) =>
    invoke<void>("set_playlist_enabled", { id, enabled }),
  setPlaylistOverwrite: (id: number, overwrite: boolean) =>
    invoke<void>("set_playlist_overwrite", { id, overwrite }),

  syncPlaylist: (id: number) => invoke<SyncReport>("sync_playlist", { id }),
  syncAll: () => invoke<SyncReport[]>("sync_all"),
  cancelSync: () => invoke<boolean>("cancel_sync"),
  pauseSync: () => invoke<boolean>("pause_sync"),
  resumeSync: () => invoke<boolean>("resume_sync"),
  getSyncControl: () =>
    invoke<{ running: boolean; paused: boolean }>("get_sync_control"),
  manualPrune: (id: number) => invoke<number>("manual_prune", { id }),

  getSyncLogs: (limit: number) =>
    invoke<
      { id: number; ts: string; playlistName: string; status: string; message: string }[]
    >("get_sync_logs", { limit }),
  listQuarantine: () => invoke<QuarantineItem[]>("list_quarantine"),
  restoreQuarantine: (id: number) => invoke<void>("restore_quarantine", { id }),
  deleteQuarantine: (id: number) => invoke<void>("delete_quarantine", { id }),

  getLikedSongs: () => invoke<Record<string, unknown>[]>("get_liked_songs"),
  getPurchasedSongs: () => invoke<Record<string, unknown>[]>("get_purchased_songs"),
  backupSongs: (
    kind: "liked" | "purchased",
    label: string,
    targetDir: string,
    quality?: string | null,
    writeLrc?: boolean | null,
    overwrite?: boolean
  ) =>
    invoke<BatchItemResult[]>("backup_songs", {
      kind,
      label,
      targetDir,
      quality,
      writeLrc,
      overwrite,
    }),
  preflightPlaylist: (id: number, force?: boolean) =>
    invoke<TrackAvailability[]>("preflight_playlist", { id, force: force ?? false }),
  previewLocalMatch: (id: number) =>
    invoke<LocalMatchPreview[]>("preview_local_match", { id }),
  previewLocalFolder: (folder: string) =>
    invoke<LocalMatchPreview[]>("preview_local_folder", { folder }),
  showInFolder: (path: string) => invoke<void>("show_in_folder", { path }),
  checkForUpdate: () => invoke<string | null>("check_for_update"),
  setPlaylistSyncPolicy: (
    id: number,
    mode: string | null,
    uploadManual: boolean | null
  ) => invoke<void>("set_playlist_sync_policy", { id, mode, uploadManual }),
  getPlaylistSettings: (id: number) =>
    invoke<{
      playlistId: number;
      modeOverride: string | null;
      uploadManual: boolean | null;
      globalMode: string;
      globalUploadManual: boolean;
    }>("get_playlist_settings", { id }),

  getAccountStats: (force?: boolean) =>
    invoke<AccountStats>("get_account_stats", { force: force ?? false }),
  getLocalStats: () => invoke<LocalStats>("get_local_stats"),

  getSyncChanges: (limit: number) =>
    invoke<SyncChangeEntry[]>("get_sync_changes", { limit }),
  getDeletedLog: (limit: number) =>
    invoke<DeletedLogEntry[]>("get_deleted_log", { limit }),
  getPlaylistHistory: (playlistId: number, limit: number) =>
    invoke<PlaylistHistoryEntry[]>("get_playlist_history", { playlistId, limit }),
  restoreDeletedItem: (id: number) => invoke<string>("restore_deleted_item", { id }),
  restorePlaylistSnapshot: (playlistId: number, historyId: number) =>
    invoke<number>("restore_playlist_snapshot_cmd", { playlistId, historyId }),
  previewPlaylistRestore: (playlistId: number, historyId: number) =>
    invoke<{ historyId: number; toAdd: Record<string, unknown>[]; toRemove: Record<string, unknown>[] }>(
      "preview_playlist_restore_cmd",
      { playlistId, historyId }
    ),
  clearSyncHistory: (kind: "logs" | "changes" | "deleted" | "history") =>
    invoke<number>("clear_sync_history_cmd", { kind }),

  convertNcmManual: (paths: string[], keepSource: boolean, overwrite: boolean) =>
    invoke<NcmConvertReport>("convert_ncm_manual", { paths, keepSource, overwrite }),
  setAutoLaunch: (enabled: boolean) => invoke<void>("set_auto_launch", { enabled }),
};
