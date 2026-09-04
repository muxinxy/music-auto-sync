import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  BatchItemResult,
  Config,
  LoginStatus,
  PlaylistInfo,
  PlaylistSongsResult,
  QuarantineItem,
  QrCheckResult,
  SingleDownloadOptions,
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

  listPlaylists: () => invoke<PlaylistInfo[]>("list_playlists"),
  getPlaylistSongs: (id: number) =>
    invoke<PlaylistSongsResult>("get_playlist_songs", { id }),
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
  preflightPlaylist: (id: number) =>
    invoke<TrackAvailability[]>("preflight_playlist", { id }),
  showInFolder: (path: string) => invoke<void>("show_in_folder", { path }),
  checkForUpdate: () => invoke<string | null>("check_for_update"),
};
