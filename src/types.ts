export interface AppInfo {
  dataDir: string;
  dataDirPortable: boolean;
  version: string;
}

export interface PlaylistSyncSetting {
  id: number;
  name: string;
  enabled: boolean;
  folderOverride?: string | null;
  qualityOverride?: string | null;
}

export interface Config {
  apiBase: string;
  musicRoot?: string | null;
  folderTemplate: string;
  filenameTemplate: string;
  quality: string;
  autoSyncOnStartup: boolean;
  syncIntervalMinutes?: number | null;
  ncmConvert: boolean;
  ncmScanDirs?: string[];
  embedCover: boolean;
  embedLyrics: boolean;
  writeLrc: boolean;
  writeM3u8: boolean;
  concurrency: number;
  playlists: PlaylistSyncSetting[];
  cookie?: string | null;
  cookieUser?: { userId: number; nickname: string } | null;
}

export interface QrSession {
  key: string;
  qrImg: string;
}

export type QrState = "waiting" | "scanned" | "success" | "expired";

export interface QrCheckResult {
  state: QrState;
  message: string;
  nickname?: string;
}

export interface LoginStatus {
  loggedIn: boolean;
  nickname?: string;
  userId?: number;
  avatarUrl?: string;
}

export interface PlaylistInfo {
  id: number;
  name: string;
  coverImgUrl: string;
  trackCount: number;
  subscribed: boolean;
  enabled: boolean;
  lastSync?: string | null;
  lastResult?: string | null;
}

export interface SyncProgress {
  playlistId?: number;
  playlistName: string;
  phase: string;
  current: number;
  total: number;
  message: string;
}

export interface SyncReport {
  playlistId: number;
  playlistName: string;
  added: number;
  updated: number;
  quarantined: number;
  ncmConverted: number;
  failed: number;
  skipped: number;
  errors: string[];
  startedAt: string;
  finishedAt: string;
}

export interface QuarantineItem {
  id: number;
  playlistName: string;
  fileName: string;
  originalPath: string;
  quarantinePath: string;
  quarantinedAt: string;
}
