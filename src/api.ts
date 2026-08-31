import { invoke } from "@tauri-apps/api/core";
import type {
  AppInfo,
  Config,
  LoginStatus,
  PlaylistInfo,
  QuarantineItem,
  QrCheckResult,
  SyncReport,
} from "./types";

export const api = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  getConfig: () => invoke<Config>("get_config"),
  saveConfig: (config: Config) => invoke<Config>("save_config", { config }),
  setDataDir: (dir: string, migrate: boolean) =>
    invoke<AppInfo>("set_data_dir", { dir, migrate }),

  getLoginQr: () => invoke<{ key: string; qrImg: string }>("get_login_qr"),
  checkLoginQr: (key: string) => invoke<QrCheckResult>("check_login_qr", { key }),
  getLoginStatus: () => invoke<LoginStatus>("get_login_status"),
  logout: () => invoke<void>("logout"),

  listPlaylists: () => invoke<PlaylistInfo[]>("list_playlists"),
  setPlaylistEnabled: (id: number, enabled: boolean) =>
    invoke<void>("set_playlist_enabled", { id, enabled }),

  syncPlaylist: (id: number) => invoke<SyncReport>("sync_playlist", { id }),
  syncAll: () => invoke<SyncReport[]>("sync_all"),
  cancelSync: () => invoke<boolean>("cancel_sync"),

  getSyncLogs: (limit: number) =>
    invoke<
      { id: number; ts: string; playlistName: string; status: string; message: string }[]
    >("get_sync_logs", { limit }),
  listQuarantine: () => invoke<QuarantineItem[]>("list_quarantine"),
  restoreQuarantine: (id: number) => invoke<void>("restore_quarantine", { id }),
  deleteQuarantine: (id: number) => invoke<void>("delete_quarantine", { id }),
};
