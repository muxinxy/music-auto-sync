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
  overwrite?: boolean;
}

export interface Config {
  apiBase: string;
  httpProxy?: string | null;
  musicRoot?: string | null;
  folderTemplate: string;
  filenameTemplate: string;
  artistSeparator: string;
  language: string;
  quality: string;
  downloadSource: string;
  autoSyncOnStartup: boolean;
  syncIntervalMinutes?: number | null;
  closeToTray: boolean;
  useRandomCnIp: boolean;
  ncmConvert: boolean;
  ncmScanDirs?: string[];
  ncmKeepSource: boolean;
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
  synced: number;
  overwrite: boolean;
  lastSync?: string | null;
  lastResult?: string | null;
}

export interface PlaylistSong {
  id: number;
  name: string;
  artists: string;
  album: string;
  durationMs: number;
  position: number;
  localPath?: string | null;
  synced: boolean;
}

export interface PlaylistSongsResult {
  playlistId: number;
  playlistName: string;
  songs: PlaylistSong[];
}

export interface UiMessage {
  code: string;
  params?: string[];
}

export interface SingleDownloadOptions {
  targetDir?: string | null;
  filenameTemplate?: string | null;
  quality?: string | null;
  writeLrc?: boolean | null;
  overwrite: boolean;
}

export interface SyncProgress {
  playlistId?: number;
  playlistName: string;
  phase: string;
  current: number;
  total: number;
  message: UiMessage;
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
  errors: UiMessage[];
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
