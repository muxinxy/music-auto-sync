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
  modeOverride?: string | null;
  uploadManual?: boolean | null;
}

export interface Config {
  apiBase: string;
  httpProxy?: string | null;
  musicRoot?: string | null;
  folderTemplate: string;
  filenameTemplate: string;
  artistSeparator: string;
  language: string;
  theme: string;
  ua: string;
  preflight: boolean;
  retry: number;
  quality: string;
  downloadSource: string;
  syncMode: string;
  uploadManual: boolean;
  autoSyncOnStartup: boolean;
  syncIntervalMinutes?: number | null;
  autoLaunch: boolean;
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
  creatorUserId?: number | null;
  enabled: boolean;
  synced: number;
  overwrite: boolean;
  lastSync?: string | null;
  lastResult?: string | null;
  modeOverride?: string | null;
  uploadManual?: boolean | null;
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
  fileSize?: number | null;
  fileModified?: string | null;
}

export interface TrackAvailability {
  id: number;
  downloadable: boolean;
  downloadLevel?: string | null;
  playLevel?: string | null;
  fee?: number | null;
  locked: boolean;
  reason?: string | null;
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
  errorDetails?: SyncErrorDetail[];
  startedAt: string;
  finishedAt: string;
}

export interface SyncErrorDetail {
  trackId: number;
  trackName: string;
  message: UiMessage;
}

export type BatchItemStatus = "downloaded" | "skipped" | "failed";

export interface BatchItemResult {
  trackId: number;
  trackName: string;
  outcome:
    | { status: "downloaded"; data: string }
    | { status: "skipped" }
    | { status: "failed"; data: UiMessage };
}

export interface QuarantineItem {
  id: number;
  playlistName: string;
  fileName: string;
  originalPath: string;
  quarantinePath: string;
  quarantinedAt: string;
}

export interface AccountStats {
  nickname?: string | null;
  userId?: number | null;
  avatarUrl?: string | null;
  level?: number | null;
  vipLevel?: number | null;
  follows?: number | null;
  followeds?: number | null;
  createdPlaylistCount?: number | null;
  subscribedPlaylistCount?: number | null;
  likedCount?: number | null;
  eventCount?: number | null;
}

export interface LocalStats {
  totalSyncRuns: number;
  totalAdded: number;
  totalQuarantined: number;
  totalNcmConverted: number;
  totalFailed: number;
  currentLocalFiles: number;
  quarantineItems: number;
  historySnapshots: number;
}

export interface SyncChangeEntry {
  id: number;
  syncRunId: number;
  ts: string;
  playlistId: number;
  playlistName: string;
  direction: string;
  action: string;
  trackId?: number | null;
  trackName?: string | null;
  localPath?: string | null;
  quarantinedPath?: string | null;
  neteaseId?: number | null;
  note?: string | null;
}

export interface DeletedLogEntry {
  id: number;
  ts: string;
  kind: "local_file" | "playlist_track";
  playlistId: number;
  playlistName: string;
  trackId?: number | null;
  trackName?: string | null;
  localPath?: string | null;
  quarantinedPath?: string | null;
  neteaseId?: number | null;
  restoredAt?: string | null;
  note?: string | null;
}

export interface PlaylistHistoryEntry {
  id: number;
  playlistId: number;
  ts: string;
  playlistName: string;
  snapshot: string;
  source: string;
}

export interface NcmConvertItemResult {
  source: string;
  output?: string | null;
  status: "converted" | "skipped" | "failed";
  error?: string | null;
}

export interface NcmConvertReport {
  converted: number;
  skipped: number;
  failed: number;
  items: NcmConvertItemResult[];
}
