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
  /** 目标为无损且本地为有损时，重新下载无损替换。 */
  upgradeQuality: boolean;
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

export interface LocalMatchPreview {
  path: string;
  fileName: string;
  neteaseId?: number | null;
  matched: boolean;
  trackName?: string | null;
  synced: boolean;
  isRegisteredFile: boolean;
  /** 命中且未登记时：文件名是否已符合当前命名模板（true = 同步时直接登记免改名）。 */
  nameMatchesTemplate: boolean;
  matchKind: "sidecar" | "key163" | "id3" | "tag" | "none";
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
  /** 本次任务的 sync_logs 行 id（= 变更流水 run id），歌单任务的任务详情据此查询。 */
  runId?: number | null;
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

export interface CloudSong {
  songId: number;
  songName: string;
  artist: string;
  album: string;
  fileName: string;
  fileSize: number;
  bitrate?: number | null;
  /** 上传时间（epoch 毫秒）。 */
  addTime?: number | null;
  /** 匹配到的网易曲目 id（未匹配为 null）。 */
  simpleSongId?: number | null;
}

export interface CloudStorageInfo {
  count: number;
  usedSize?: number | null;
  maxSize?: number | null;
}

export interface CloudListResult {
  songs: CloudSong[];
  storage: CloudStorageInfo;
}

export interface CloudUploadItem {
  path: string;
  fileName: string;
  fileSize: number;
  neteaseId: number;
  title: string;
  artist: string;
  album: string;
}

export interface CloudUploadPlan {
  uploads: CloudUploadItem[];
  /** 无法匹配网易曲目而将被跳过的本地文件路径。 */
  unresolved: string[];
  cloudCount: number;
  localCount: number;
}

/** 云盘任务明细行（事件 cloud://row，按 path 合并）。result 取值见后端 CloudTaskRow。 */
export interface CloudTaskRow {
  path: string;
  fileName: string;
  title: string;
  artist: string;
  album: string;
  neteaseId: number;
  fileSize: number;
  result:
    | "in_cloud"
    | "duplicate"
    | "unresolved"
    | "to_upload"
    | "uploading"
    | "uploaded"
    | "instant"
    | "downloading"
    | "downloaded"
    | "dl_skipped"
    | "failed";
  message?: UiMessage | null;
}

/** 某次同步任务的变更记录（get_run_changes，含可恢复 id）。 */
export interface RunChangeEntry {
  id: number;
  ts: string;
  playlistName: string;
  direction: string;
  action: string;
  trackId?: number | null;
  trackName?: string | null;
  localPath?: string | null;
  quarantinedPath?: string | null;
  neteaseId?: number | null;
  note?: string | null;
  restoreId?: number | null;
  restoreKind?: "deleted" | "quarantine" | null;
}

export interface CloudDownloadItem {
  id: number;
  fileName: string;
}
