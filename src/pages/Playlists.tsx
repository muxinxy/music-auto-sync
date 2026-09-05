import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Avatar,
  Button,
  Card,
  Checkbox,
  Drawer,
  Dropdown,
  Input,
  List,
  Modal,
  Popconfirm,
  Progress,
  Segmented,
  Select,
  Space,
  Spin,
  Switch,
  Table,
  Tag,
  Tooltip,
  Typography,
  message as antMessage,
} from "antd";
import {
  CloudDownloadOutlined,
  DownloadOutlined,
  EyeOutlined,
  FolderOpenOutlined,
  HeartOutlined,
  HistoryOutlined,
  ReloadOutlined,
  ShoppingOutlined,
  SyncOutlined,
} from "@ant-design/icons";
import type { ColumnsType } from "antd/es/table";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { formatError, translateUi, uiMessage } from "../errors";
import type {
  BatchItemResult,
  LocalMatchPreview,
  LoginStatus,
  PlaylistHistoryEntry,
  PlaylistInfo,
  PlaylistSong,
  PlaylistSongsResult,
  SingleDownloadOptions,
  SyncReport,
  TrackAvailability,
  UiMessage,
} from "../types";
import type { SyncEventState } from "../App";

interface Props {
  login: LoginStatus | null;
  sync: SyncEventState;
}

function formatDuration(millis: number): string {
  if (!millis) return "-";
  const total = Math.round(millis / 1000);
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function formatBytes(bytes?: number | null): string {
  if (bytes == null) return "-";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

function displayLastResult(raw?: string | null): string {
  if (!raw) return "";
  if (raw.startsWith("{")) {
    try {
      return translateUi(JSON.parse(raw) as UiMessage);
    } catch {
      return raw;
    }
  }
  return raw;
}

const QUALITY_OPTIONS = ["standard", "higher", "exhigh", "lossless", "hires"] as const;
const VARIABLE_HINT = "{音轨号} {歌手} {标题} {专辑} {网易云ID}";
const CACHE_TTL_MS = 60_000;
/** 歌单列表缓存：key 绑定登录账号，账号切换立即视为 miss，避免展示上一账号数据。 */
const playlistCache: { userId: number | null; at: number; data: PlaylistInfo[] } = {
  userId: null,
  at: 0,
  data: [],
};

export default function PlaylistsPage({ login, sync }: Props) {
  const { t } = useTranslation();
  const [playlists, setPlaylists] = useState<PlaylistInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState("");
  const [group, setGroup] = useState<"all" | "created" | "subscribed">("all");
  const [detailId, setDetailId] = useState<number | null>(null);
  const [songs, setSongs] = useState<PlaylistSongsResult | null>(null);
  const [songsLoading, setSongsLoading] = useState(false);
  const [availability, setAvailability] = useState<Record<number, TrackAvailability>>({});
  const [availabilityLoading, setAvailabilityLoading] = useState(false);

  const [selectedPlaylists, setSelectedPlaylists] = useState<Set<number>>(new Set());
  const [selectedSongs, setSelectedSongs] = useState<number[]>([]);
  const [detailPolicy, setDetailPolicy] = useState<{
    mode: string;
    uploadManual: boolean | null;
  } | null>(null);
  const [historyList, setHistoryList] = useState<PlaylistHistoryEntry[]>([]);
  const [matchOpen, setMatchOpen] = useState(false);
  const [matchList, setMatchList] = useState<LocalMatchPreview[]>([]);
  const [matchLoading, setMatchLoading] = useState(false);
  const [matchPlaylistId, setMatchPlaylistId] = useState<number | null>(null);

  const [dlTarget, setDlTarget] = useState<{
    playlistId: number;
    songs: PlaylistSong[];
  } | null>(null);
  const [dlOptions, setDlOptions] = useState<SingleDownloadOptions>({ overwrite: false });
  const [dlDownloading, setDlDownloading] = useState(false);
  const [dlFailures, setDlFailures] = useState<{ song: PlaylistSong; message: UiMessage }[]>([]);
  const [lastReport, setLastReport] = useState<SyncReport | null>(null);
  const dlPauseRef = useRef(false);
  const dlCancelRef = useRef(false);
  const [dlPaused, setDlPaused] = useState(false);

  const load = useCallback(
    async (force = false) => {
      if (!login?.loggedIn) return;
      const userId = login.userId ?? null;
      const cacheFresh =
        playlistCache.userId === userId &&
        !force &&
        Date.now() - playlistCache.at < CACHE_TTL_MS &&
        playlistCache.data.length > 0;
      if (cacheFresh) {
        setPlaylists(playlistCache.data);
        return;
      }
      setLoading(true);
      try {
        // force=true（刷新按钮/操作后重载）时穿透后端 TTL 缓存直拉网易。
        const data = await api.listPlaylists(force);
        playlistCache.userId = userId;
        playlistCache.at = Date.now();
        playlistCache.data = data;
        setPlaylists(data);
      } catch (e) {
        antMessage.error(t("playlists.loadFailed", { detail: formatError(e) }));
      } finally {
        setLoading(false);
      }
    },
    [login?.loggedIn, login?.userId, t]
  );

  useEffect(() => {
    load();
  }, [load]);

  const openDetail = useCallback(async (id: number) => {
    setDetailId(id);
    setSongs(null);
    setAvailability({});
    setLastReport(null);
    // 首次打开不穿透缓存（普通读取）；抽屉内“刷新”按钮穿透。
    await loadDetail(id, false);
    // 从后端读取该歌单的同步策略（覆盖值 + 全局默认），避免依赖列表缓存。
    api
      .getPlaylistSettings(id)
      .then((s) =>
        setDetailPolicy({
          mode: s.modeOverride ?? "",
          uploadManual: s.uploadManual ?? null,
        })
      )
      .catch(() =>
        setDetailPolicy({
          mode: "",
          uploadManual: null,
        })
      );
    api
      .getPlaylistHistory(id, 50)
      .then((h) => setHistoryList(h))
      .catch(() => setHistoryList([]));
  }, []);

  /** 拉取歌单歌曲列表 + 后台预检。force=true 穿透后端 TTL 缓存（刷新按钮用）。 */
  const loadDetail = useCallback(async (id: number, force: boolean) => {
    setSongsLoading(true);
    setAvailabilityLoading(true);
    try {
      const result = await api.getPlaylistSongs(id, force);
      setSongs(result);
      api
        .preflightPlaylist(id, force)
        .then((list) => {
          const map: Record<number, TrackAvailability> = {};
          for (const item of list) map[item.id] = item;
          setAvailability(map);
        })
        .catch(() => {})
        .finally(() => setAvailabilityLoading(false));
    } catch (e) {
      antMessage.error(t("playlists.loadSongsFailed", { detail: formatError(e) }));
      setDetailId(null);
    } finally {
      setSongsLoading(false);
    }
  }, [t]);

  const openDownloadDialog = (playlistId: number, songs: PlaylistSong[]) => {
    dlPauseRef.current = false;
    dlCancelRef.current = false;
    setDlPaused(false);
    setDlTarget({ playlistId, songs });
    setDlFailures([]);
    setDlOptions({
      overwrite: songs.length === 1 ? songs[0].synced : false,
      writeLrc: false,
    });
  };

  const pickDownloadDir = async () => {
    const path = (await open({ directory: true, multiple: false, title: t("playlists.choose") })) as
      | string
      | null;
    if (path) setDlOptions((o) => ({ ...o, targetDir: path }));
  };

  const confirmDownload = async () => {
    if (!dlTarget) return;
    const { playlistId, songs: targets } = dlTarget;
    setDlDownloading(true);
    dlCancelRef.current = false;
    const failures: { song: PlaylistSong; message: UiMessage }[] = [];
    let canceled = false;
    try {
      for (const song of targets) {
        if (dlCancelRef.current) {
          canceled = true;
          break;
        }
        // 暂停等待（可继续/取消）。
        while (dlPauseRef.current) {
          if (dlCancelRef.current) {
            canceled = true;
            break;
          }
          await new Promise((r) => setTimeout(r, 200));
        }
        if (canceled) break;
        try {
          await api.downloadSongWithOptions(playlistId, song.id, {
            ...dlOptions,
            writeLrc: dlOptions.writeLrc ?? false,
          });
        } catch (e) {
          failures.push({ song, message: uiMessage(e) });
        }
      }
      setDlFailures(failures);
      const ok = targets.length - failures.length;
      if (canceled) {
        antMessage.info(t("playlists.downloadCanceled", { done: ok, total: targets.length }));
      } else if (targets.length === 1) {
        if (failures.length === 0) {
          antMessage.success(t("playlists.downloaded", { name: targets[0].name }));
        } else {
          antMessage.error(t("playlists.downloadFailed", { detail: formatError(failures[0].message) }));
        }
      } else {
        if (ok > 0) antMessage.success(t("playlists.batchDownloaded", { ok }));
        if (failures.length > 0) {
          const sample = failures
            .slice(0, 3)
            .map((f) => `${f.song.name}：${formatError(f.message)}`)
            .join("；");
          antMessage.error(t("playlists.batchFailedList", { failed: failures.length, detail: sample }));
        }
      }
      if (canceled || failures.length === 0) {
        setDlTarget(null);
        setSelectedSongs([]);
      }
      setSongs(await api.getPlaylistSongs(playlistId, true));
      load(true);
    } finally {
      setDlDownloading(false);
      dlPauseRef.current = false;
      setDlPaused(false);
    }
  };

  const retryFailedOnly = async () => {
    if (!dlTarget) return;
    const failedSongs = dlFailures.map((f) => f.song);
    if (failedSongs.length === 0) return;
    // 只把失败项作为目标重试；成功项留在列表里不再重复下载。
    setDlTarget({ playlistId: dlTarget.playlistId, songs: failedSongs });
    setDlFailures([]);
    await confirmDownload();
    // confirmDownload 成功清零时会把弹窗关闭；失败时保留弹窗。
  };

  const saveDetailPolicy = async () => {
    if (!detailId || !detailPolicy) return;
    const mode = detailPolicy.mode === "" ? null : detailPolicy.mode;
    try {
      await api.setPlaylistSyncPolicy(detailId, mode, detailPolicy.uploadManual);
      antMessage.success(t("settings.saved"));
      // 重新拉取列表并回填保存后的实际覆盖值（仅改本地 config 策略，无需穿透远端缓存）。
      const fresh = await api.listPlaylists(false);
      playlistCache.userId = login?.userId ?? null;
      playlistCache.at = Date.now();
      playlistCache.data = fresh;
      setPlaylists(fresh);
      const pl = fresh.find((p) => p.id === detailId);
      setDetailPolicy({
        mode: pl?.modeOverride ?? "",
        uploadManual: pl?.uploadManual ?? null,
      });
    } catch (e) {
      antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
    }
  };

  /** 打开本地匹配预览：给 playlistId 则预选该歌单，否则用列表中第一个。 */
  const openMatchPreview = async (playlistId?: number) => {
    const target = playlistId ?? playlists[0]?.id;
    if (target == null) {
      antMessage.info(t("playlists.matchPreviewNoPlaylist"));
      return;
    }
    setMatchPlaylistId(target);
    setMatchOpen(true);
    setMatchLoading(true);
    setMatchList([]);
    try {
      const list = await api.previewLocalMatch(target);
      setMatchList(list);
    } catch (e) {
      antMessage.error(t("playlists.matchPreviewFailed", { detail: formatError(e) }));
      setMatchOpen(false);
    } finally {
      setMatchLoading(false);
    }
  };

  /** 切换预览目标歌单。 */
  const changeMatchPlaylist = async (id: number) => {
    setMatchPlaylistId(id);
    setMatchLoading(true);
    setMatchList([]);
    try {
      setMatchList(await api.previewLocalMatch(id));
    } catch (e) {
      antMessage.error(t("playlists.matchPreviewFailed", { detail: formatError(e) }));
    } finally {
      setMatchLoading(false);
    }
  };

  const [restoreDiff, setRestoreDiff] = useState<{
    historyId: number;
    toAdd: { id?: number; name?: string }[];
    toRemove: { id?: number; name?: string }[];
  } | null>(null);
  const [restoreBusy, setRestoreBusy] = useState(false);

  const restoreHistory = async (historyId: number) => {
    if (!detailId) return;
    try {
      // 先计算差异预览，让用户确认将新增/移除哪些曲目。
      const diff = await api.previewPlaylistRestore(detailId, historyId);
      setRestoreDiff({
        historyId: diff.historyId,
        toAdd: diff.toAdd.map((v) => ({ id: v.id as number, name: v.name as string })),
        toRemove: diff.toRemove.map((v) => ({ id: v.id as number, name: v.name as string })),
      });
    } catch (e) {
      antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
    }
  };

  const doRestore = async () => {
    if (!detailId || !restoreDiff) return;
    setRestoreBusy(true);
    try {
      const count = await api.restorePlaylistSnapshot(detailId, restoreDiff.historyId);
      antMessage.success(t("playlists.restoreSnapshotDone", { count }));
      setRestoreDiff(null);
      load(true);
      api
        .getPlaylistHistory(detailId, 50)
        .then((h) => setHistoryList(h))
        .catch(() => {});
    } catch (e) {
      antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
    } finally {
      setRestoreBusy(false);
    }
  };

  const runSync = async (id: number, messageName: string) => {
    try {
      const report = await api.syncPlaylist(id);
      antMessage.success(t("playlists.playlistSynced", { name: messageName }));
      if (report.failed > 0) {
        setLastReport(report);
        antMessage.warning(t("playlists.syncDoneWithErrors", { failed: report.failed }));
      }
      setSongs(await api.getPlaylistSongs(id, true));
      load(true);
    } catch (e) {
      antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
    }
  };

  const openBackup = async (kind: "liked" | "purchased") => {
    const dir = (await open({
      directory: true,
      multiple: false,
      title: t("playlists.backupChooseDir"),
    })) as string | null;
    if (!dir) return;
    antMessage.loading({ key: "backup", content: t("playlists.backupRunning"), duration: 0 });
    try {
      const label =
        kind === "liked" ? t("app.menu.likedShort") : t("app.menu.purchasedShort");
      const results = await api.backupSongs(kind, label, dir);
      const ok = results.filter((r) => r.outcome.status === "downloaded").length;
      const skipped = results.filter((r) => r.outcome.status === "skipped").length;
      const failed = results.filter((r) => r.outcome.status === "failed").length;
      antMessage.destroy("backup");
      antMessage.success(t("playlists.backupDone", { ok, skipped, failed }));
    } catch (e) {
      antMessage.destroy("backup");
      antMessage.error(t("playlists.backupEmpty") + `：${formatError(e)}`);
    }
  };

  if (!login?.loggedIn) {
    return (
      <div style={{ padding: 24 }}>
        <Alert message={t("playlists.needLogin")} type="warning" showIcon />
      </div>
    );
  }

  const myId = login?.userId;
  const shown = playlists.filter((p) => {
    const owned = p.creatorUserId != null ? p.creatorUserId === myId : !p.subscribed;
    if (group === "created" && !owned) return false;
    if (group === "subscribed" && owned) return false;
    return p.name.toLowerCase().includes(filter.toLowerCase());
  });

  const reasonTag = (song: PlaylistSong) => {
    const avail = availability[song.id];
    if (!avail) return null;
    if (avail.locked) {
      return <Tag color="red">{t("playlists.lockedTag")}</Tag>;
    }
    if (!avail.downloadable) {
      const reasonKey = avail.reason ?? "locked";
      return (
        <Tooltip title={t(`playlists.reason.${reasonKey}`, { defaultValue: reasonKey })}>
          <Tag color="orange">{t("playlists.downloadLimited")}</Tag>
        </Tooltip>
      );
    }
    if (avail.downloadLevel && avail.downloadLevel !== "standard") {
      return <Tag color="cyan">{t("playlists.qualityLimitHint", { level: avail.downloadLevel })}</Tag>;
    }
    return null;
  };

  const columns: ColumnsType<PlaylistSong> = [
    {
      title: t("playlists.columns.no"),
      dataIndex: "position",
      width: 48,
      render: (value: number) => <Typography.Text type="secondary">{value}</Typography.Text>,
    },
    {
      title: t("playlists.columns.song"),
      dataIndex: "name",
      ellipsis: true,
      render: (value: string, song) => (
        <Space size={4}>
          <Tooltip title={value}>
            <span style={{ display: "inline-block", maxWidth: 240, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", verticalAlign: "bottom" }}>
              {value}
            </span>
          </Tooltip>
          {reasonTag(song)}
        </Space>
      ),
    },
    {
      title: t("playlists.columns.artists"),
      dataIndex: "artists",
      ellipsis: true,
      render: (value: string) => (
        <Tooltip title={value}>
          <span style={{ display: "inline-block", maxWidth: "100%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", verticalAlign: "bottom" }}>
            {value}
          </span>
        </Tooltip>
      ),
    },
    {
      title: t("playlists.columns.album"),
      dataIndex: "album",
      ellipsis: true,
      render: (value: string) => (
        <Tooltip title={value || "-"}>
          <span style={{ display: "inline-block", maxWidth: "100%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", verticalAlign: "bottom" }}>
            {value || "-"}
          </span>
        </Tooltip>
      ),
    },
    {
      title: t("playlists.columns.duration"),
      dataIndex: "durationMs",
      width: 72,
      render: (value: number) => formatDuration(value),
    },
    {
      title: t("playlists.columns.status"),
      dataIndex: "synced",
      width: 120,
      render: (synced: boolean, song) => {
        if (!song.localPath) {
          return <Tag>{t("playlists.unsyncedTag")}</Tag>;
        }
        return synced ? (
          <Tag color="success">{t("playlists.syncedTag")}</Tag>
        ) : (
          <Tag color="warning">{t("playlists.missingTag")}</Tag>
        );
      },
    },
    {
      title: t("playlists.sizeCol"),
      key: "fileSize",
      width: 90,
      render: (_, song) => formatBytes(song.fileSize),
    },
    {
      title: t("playlists.columns.action"),
      key: "action",
      width: 150,
      render: (_, song) => (
        <Space size={4}>
          <Button
            size="small"
            icon={<DownloadOutlined />}
            disabled={dlDownloading}
            onClick={() => detailId && openDownloadDialog(detailId, [song])}
          >
            {t("playlists.download")}
          </Button>
          {song.synced && song.localPath && (
            <Tooltip title={t("playlists.showInFolder")}>
              <Button
                size="small"
                icon={<FolderOpenOutlined />}
                onClick={() => api.showInFolder(song.localPath!)}
              />
            </Tooltip>
          )}
        </Space>
      ),
    },
  ];

  const allOverwrite = playlists.length > 0 && playlists.every((p) => p.overwrite);

  const togglePlaylist = (id: number) => {
    setSelectedPlaylists((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const toggleAllPlaylists = () => {
    setSelectedPlaylists((prev) =>
      prev.size === shown.length && shown.every((p) => prev.has(p.id))
        ? new Set()
        : new Set(shown.map((p) => p.id))
    );
  };

  const batchPlaylists = async (
    operation: "enabled" | "disabled" | "overwrite" | "noOverwrite" | "sync"
  ) => {
    const ids = [...selectedPlaylists];
    try {
      for (const id of ids) {
        if (operation === "sync") {
          await api.syncPlaylist(id);
        } else if (operation === "enabled" || operation === "disabled") {
          await api.setPlaylistEnabled(id, operation === "enabled");
        } else {
          await api.setPlaylistOverwrite(id, operation === "overwrite");
        }
      }
      antMessage.success(`${ids.length} ${t("playlists.selectedCount", { count: ids.length })}`);
      setSelectedPlaylists(new Set());
      load(true);
    } catch (e) {
      antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
    }
  };

  const openBatchDownloadSongs = () => {
    if (!detailId) return;
    const targets = (songs?.songs ?? []).filter((song) => selectedSongs.includes(song.id));
    if (targets.length === 0) return;
    setSelectedSongs([]);
    openDownloadDialog(detailId, targets);
  };

  const selectedPlaylistSongsAll = () => {
    if (!detailId || !songs) return;
    const targets = songs.songs.filter((s) => !s.synced);
    if (targets.length === 0) {
      antMessage.info(t("playlists.noMissingSongs"));
      return;
    }
    openDownloadDialog(detailId, targets);
  };

  const reportErrorList = lastReport?.errorDetails?.length
    ? lastReport.errorDetails
    : (lastReport?.errors ?? []).map((m, i) => ({
        trackId: 0,
        trackName: String(i + 1),
        message: m,
      }));

  return (
    <div style={{ padding: 24 }}>
      <Card style={{ marginBottom: 16 }}>
        <Space style={{ width: "100%", justifyContent: "space-between" }} wrap>
          <Space>
            <Space size={4}>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {t("playlists.overwriteAll")}
              </Typography.Text>
              <Switch
                size="small"
                checked={allOverwrite}
                onChange={async (value) => {
                  for (const playlist of playlists) {
                    await api.setPlaylistOverwrite(playlist.id, value);
                  }
                  load(true);
                }}
              />
            </Space>
            <Input.Search
              placeholder={t("playlists.searchPlaceholder")}
              allowClear
              style={{ width: 220 }}
              onSearch={setFilter}
              onChange={(e) => !e.target.value && setFilter("")}
            />
            <Segmented
              value={group}
              onChange={(v) => setGroup(v as "all" | "created" | "subscribed")}
              options={[
                { value: "all", label: t("playlists.groupAll") },
                { value: "created", label: t("playlists.groupCreated") },
                { value: "subscribed", label: t("playlists.groupSubscribed") },
              ]}
            />
            <Typography.Text type="secondary">
              {t("playlists.countHint", { count: shown.length })}
            </Typography.Text>
          </Space>
          <Space>
            <Button icon={<ReloadOutlined />} onClick={() => load(true)} disabled={sync.running}>
              {t("playlists.refresh")}
            </Button>
            <Button icon={<FolderOpenOutlined />} onClick={() => openMatchPreview()} disabled={sync.running}>
              {t("playlists.matchPreviewGlobal")}
            </Button>
            <Dropdown
              menu={{
                items: [
                  { key: "liked", label: t("playlists.backupLiked"), icon: <HeartOutlined /> },
                  { key: "purchased", label: t("playlists.backupPurchased"), icon: <ShoppingOutlined /> },
                ],
                onClick: ({ key }) => openBackup(key as "liked" | "purchased"),
              }}
            >
              <Button icon={<CloudDownloadOutlined />}>{t("playlists.backupTitle")}</Button>
            </Dropdown>
            <Button
              type="primary"
              icon={<SyncOutlined spin={sync.running} />}
              loading={sync.running}
              onClick={async () => {
                try {
                  const reports = await api.syncAll();
                  const failedTotal = reports.reduce((sum, r) => sum + r.failed, 0);
                  if (failedTotal > 0) {
                    antMessage.warning(t("playlists.syncDoneWithErrors", { failed: failedTotal }));
                  } else {
                    antMessage.success(t("playlists.allSynced"));
                  }
                  load(true);
                } catch (e) {
                  antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
                }
              }}
            >
              {t("playlists.syncAll")}
            </Button>
          </Space>
        </Space>
      </Card>

      {selectedPlaylists.size > 0 && (
        <Card size="small" style={{ marginBottom: 16 }}>
          <Space wrap>
            <Checkbox
              checked={selectedPlaylists.size === shown.length}
              indeterminate={selectedPlaylists.size > 0 && selectedPlaylists.size < shown.length}
              onChange={toggleAllPlaylists}
            >
              {t("playlists.selectAll")}
            </Checkbox>
            <Typography.Text type="secondary">
              {t("playlists.selectedCount", { count: selectedPlaylists.size })}
            </Typography.Text>
            <Button size="small" onClick={() => batchPlaylists("enabled")}>
              {t("settings.on")}
            </Button>
            <Button size="small" onClick={() => batchPlaylists("disabled")}>
              {t("settings.off")}
            </Button>
            <Button size="small" onClick={() => batchPlaylists("overwrite")}>
              {t("playlists.overwriteShort")}
            </Button>
            <Button size="small" onClick={() => batchPlaylists("noOverwrite")}>
              {t("playlists.noOverwriteShort")}
            </Button>
            <Button
              size="small"
              type="primary"
              icon={<SyncOutlined />}
              disabled={sync.running}
              onClick={() => batchPlaylists("sync")}
            >
              {t("playlists.syncNow")}
            </Button>
            <Button size="small" type="text" onClick={() => setSelectedPlaylists(new Set())}>
              {t("playlists.cancelSelect")}
            </Button>
          </Space>
        </Card>
      )}

      <Card styles={{ body: { padding: 0 } }}>
        <List
          loading={loading}
          dataSource={shown}
          pagination={{ pageSize: 15, showSizeChanger: true, pageSizeOptions: [15, 30, 60], showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}` }}
          renderItem={(p) => {
            const percent = p.trackCount ? Math.round((p.synced / p.trackCount) * 100) : 0;
            const lastResult = displayLastResult(p.lastResult);
            return (
              <List.Item
                actions={[
                  <Switch
                    key="ow"
                    size="small"
                    checked={p.overwrite}
                    onChange={async (v) => {
                      await api.setPlaylistOverwrite(p.id, v);
                      load(true);
                    }}
                    checkedChildren={t("playlists.overwriteShort")}
                    unCheckedChildren={t("playlists.overwriteShort")}
                  />,
                  <Button
                    key="detail"
                    size="small"
                    icon={<EyeOutlined />}
                    onClick={() => openDetail(p.id)}
                  >
                    {t("playlists.view")}
                  </Button>,
                  <Switch
                    key="sw"
                    checked={p.enabled}
                    checkedChildren={t("settings.on")}
                    unCheckedChildren={t("settings.off")}
                    onChange={async (v) => {
                      await api.setPlaylistEnabled(p.id, v);
                      load(true);
                    }}
                  />,
                  <Button
                    key="go"
                    size="small"
                    disabled={sync.running}
                    onClick={() => runSync(p.id, p.name)}
                  >
                    {t("playlists.syncNow")}
                  </Button>,
                ]}
              >
                <Checkbox
                  checked={selectedPlaylists.has(p.id)}
                  onChange={() => togglePlaylist(p.id)}
                  style={{ marginRight: 8, alignSelf: "center" }}
                />
                <List.Item.Meta
                  avatar={<Avatar shape="square" size={48} src={p.coverImgUrl} />}
                  title={
                    <Space>
                      <span>{p.name}</span>
                      {p.subscribed && <Tag>{t("playlists.favorited")}</Tag>}
                    </Space>
                  }
                  description={
                    <Space direction="vertical" size={2} style={{ width: "100%", maxWidth: 420 }}>
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        {t("playlists.trackCount", { count: p.trackCount })} ·{" "}
                        {t("playlists.syncedCount", { synced: p.synced, total: p.trackCount })}
                        {p.lastSync
                          ? ` · ${t("playlists.lastSync", { time: p.lastSync })}`
                          : ` · ${t("playlists.neverSynced")}`}
                        {lastResult ? ` · ${lastResult}` : ""}
                      </Typography.Text>
                      <Progress
                        percent={percent}
                        size="small"
                        strokeColor="#c20c0c"
                        style={{ width: 240 }}
                      />
                    </Space>
                  }
                />
              </List.Item>
            );
          }}
        />
      </Card>

      <Drawer
        title={
          songs
            ? t("playlists.drawerCount", { name: songs.playlistName, count: songs.songs.length })
            : t("playlists.songListFallback")
        }
        width={920}
        open={detailId !== null}
        onClose={() => setDetailId(null)}
        extra={
          <Space>
            <Tooltip title={t("playlists.refreshSongsTip")}>
              <Button
                icon={<ReloadOutlined />}
                loading={songsLoading}
                disabled={detailId == null || sync.running}
                onClick={() => detailId != null && loadDetail(detailId, true).then(() => load(true))}
              >
                {t("playlists.refreshSongs")}
              </Button>
            </Tooltip>
            <Button
              icon={<DownloadOutlined />}
              disabled={selectedSongs.length === 0 || dlDownloading}
              onClick={openBatchDownloadSongs}
            >
              {t("playlists.downloadBatch", { count: selectedSongs.length })}
            </Button>
            <Tooltip title={t("playlists.downloadMissingTip")}>
              <Button
                icon={<CloudDownloadOutlined />}
                disabled={dlDownloading}
                onClick={selectedPlaylistSongsAll}
              >
                {t("playlists.downloadMissing")}
              </Button>
            </Tooltip>
            <Tooltip title={t("playlists.syncThisPlaylistTip")}>
              <Button
                type="primary"
                icon={<SyncOutlined />}
                disabled={sync.running}
                onClick={() => detailId && runSync(detailId, songs?.playlistName ?? "")}
              >
                {t("playlists.syncThisPlaylist")}
              </Button>
            </Tooltip>
          </Space>
        }
      >
        <Card size="small" style={{ marginBottom: 12 }}>
          <Space wrap>
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {t("playlists.syncPolicy")}
            </Typography.Text>
            <Select
              size="small"
              style={{ width: 140 }}
              value={detailPolicy?.mode ?? ""}
              onChange={(v) => setDetailPolicy((p) => ({ ...(p ?? { uploadManual: null }), mode: v }))}
              options={[
                { value: "", label: t("playlists.policyDefault") },
                { value: "mirror", label: t("settings.modeMirror") },
                { value: "add_only", label: t("settings.modeAddOnly") },
                { value: "delete_only", label: t("settings.modeDeleteOnly") },
              ]}
            />
            <Select
              size="small"
              style={{ width: 170 }}
              value={detailPolicy?.uploadManual === null ? "" : detailPolicy?.uploadManual ? "on" : "off"}
              onChange={(v) =>
                setDetailPolicy((p) => ({
                  ...(p ?? { mode: "" }),
                  uploadManual: v === "" ? null : v === "on",
                }))
              }
              options={[
                { value: "", label: t("playlists.policyDefault") },
                { value: "on", label: t("settings.labelUploadManualShort") },
                { value: "off", label: t("playlists.uploadOff") },
              ]}
            />
            <Button size="small" type="primary" ghost onClick={saveDetailPolicy}>
              {t("playlists.savePolicy")}
            </Button>
            <Button size="small" icon={<FolderOpenOutlined />} onClick={() => openMatchPreview(detailId ?? undefined)}>
              {t("playlists.matchPreview")}
            </Button>
            {historyList.length > 0 && (
              <Dropdown
                menu={{
                  items: historyList.map((h) => ({
                    key: String(h.id),
                    label: `${h.ts}（${t(`syncPage.source.${h.source}`, { defaultValue: h.source })}）`,
                  })),
                  onClick: ({ key }) => restoreHistory(Number(key)),
                }}
              >
                <Button size="small" icon={<HistoryOutlined />}>
                  {t("playlists.restoreSnapshot")}
                </Button>
              </Dropdown>
            )}
          </Space>
        </Card>
        <Table<PlaylistSong>
          rowKey="id"
          size="small"
          loading={songsLoading}
          dataSource={songs?.songs ?? []}
          columns={columns}
          rowSelection={{
            selectedRowKeys: selectedSongs,
            onChange: (keys) => setSelectedSongs(keys as number[]),
          }}
          pagination={{ pageSize: 50, showSizeChanger: false }}
        />
      </Drawer>

      <Modal
        title={t("playlists.matchPreviewTitle", {
          name: songs?.playlistName ?? "",
          count: matchList.length,
        })}
        open={matchOpen}
        width={880}
        zIndex={1100}
        onCancel={() => setMatchOpen(false)}
        footer={null}
      >
        <Space style={{ marginBottom: 12 }} wrap>
          <Select<number>
            style={{ width: 320 }}
            placeholder={t("playlists.matchPickPlaylist")}
            value={matchPlaylistId ?? undefined}
            onChange={(v) => changeMatchPlaylist(v)}
            showSearch
            optionFilterProp="label"
            options={playlists.map((p) => ({
              value: p.id,
              label: p.name,
            }))}
          />
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {t("playlists.matchCount", { count: matchList.length })}
          </Typography.Text>
        </Space>
        {matchLoading ? (
          <div style={{ textAlign: "center", padding: 24 }}>
            <Spin />
          </div>
        ) : (
          <>
            <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
              {t("playlists.matchPreviewHint")}
            </Typography.Paragraph>
            <Table<LocalMatchPreview>
              size="small"
              rowKey={(r) => r.path}
              dataSource={matchList}
              pagination={matchList.length > 10 ? { pageSize: 10, showSizeChanger: false } : false}
              columns={[
                {
                  title: t("playlists.matchColFile"),
                  dataIndex: "path",
                  ellipsis: true,
                  render: (v: string) => <Typography.Text style={{ fontSize: 12 }}>{v}</Typography.Text>,
                },
                {
                  title: t("playlists.matchColKind"),
                  dataIndex: "matchKind",
                  width: 110,
                  render: (k: LocalMatchPreview["matchKind"]) => (
                    <Tag
                      color={
                        k === "sidecar" || k === "key163"
                          ? "blue"
                          : k === "tag"
                            ? "green"
                            : k === "id3"
                              ? "purple"
                              : "default"
                      }
                    >
                      {t(`playlists.matchKind.${k}`)}
                    </Tag>
                  ),
                },
                {
                  title: t("playlists.matchColResult"),
                  key: "result",
                  width: 240,
                  render: (_, r) => {
                    if (!r.neteaseId) {
                      return <Typography.Text type="secondary" style={{ fontSize: 12 }}>-</Typography.Text>;
                    }
                    if (!r.matched) {
                      return <Tag color="default">{t("playlists.matchNotInPlaylist")}</Tag>;
                    }
                    return (
                      <Space size={4} wrap>
                        <Tag color="green">{t("playlists.matchMatched")}</Tag>
                        <Typography.Text style={{ fontSize: 12 }}>{r.trackName}</Typography.Text>
                      </Space>
                    );
                  },
                },
                {
                  title: t("playlists.matchColStatus"),
                  key: "status",
                  width: 160,
                  render: (_, r) => {
                    if (!r.neteaseId) return <Typography.Text type="secondary" style={{ fontSize: 12 }}>-</Typography.Text>;
                    if (r.synced) return <Tag color="success">{t("playlists.matchSynced")}</Tag>;
                    if (r.isRegisteredFile) return <Tag color="warning">{t("playlists.matchFileGone")}</Tag>;
                    if (r.matched) return <Tag color="processing">{t("playlists.matchPendingRename")}</Tag>;
                    return <Tag>{t("playlists.matchExtra")}</Tag>;
                  },
                },
              ]}
              locale={{ emptyText: t("playlists.matchEmpty") }}
            />
          </>
        )}
      </Modal>

      <Modal
        title={
          dlTarget && dlTarget.songs.length > 1
            ? t("playlists.downloadTitleBatch", { count: dlTarget.songs.length })
            : t("playlists.downloadTitle", { name: dlTarget?.songs[0]?.name ?? "" })
        }
        open={dlTarget !== null}
        width={620}
        onCancel={() => {
          if (dlDownloading) {
            dlCancelRef.current = true;
            dlPauseRef.current = false;
            setDlPaused(false);
          } else {
            setDlTarget(null);
          }
        }}
        footer={
          <div style={{ display: "flex", justifyContent: "flex-end" }}>
            <Space wrap style={{ justifyContent: "flex-end" }}>
            {dlDownloading ? (
              <>
                {dlPaused ? (
                  <Button
                    onClick={() => {
                      dlPauseRef.current = false;
                      setDlPaused(false);
                    }}
                  >
                    {t("app.resume")}
                  </Button>
                ) : (
                  <Button
                    onClick={() => {
                      dlPauseRef.current = true;
                      setDlPaused(true);
                    }}
                  >
                    {t("app.pause")}
                  </Button>
                )}
                <Popconfirm
                  title={t("app.cancelConfirm")}
                  okText={t("app.cancel")}
                  cancelText={t("playlists.cancel")}
                  onConfirm={() => {
                    dlCancelRef.current = true;
                    dlPauseRef.current = false;
                    setDlPaused(false);
                  }}
                >
                  <Button danger>{t("app.cancelTask")}</Button>
                </Popconfirm>
              </>
            ) : dlFailures.length > 0 ? (
              <>
                <Button onClick={() => setDlTarget(null)}>{t("playlists.cancel")}</Button>
                <Button type="primary" onClick={retryFailedOnly}>
                  {t("playlists.retryFailed")}
                </Button>
              </>
            ) : (
              <>
                <Button onClick={() => setDlTarget(null)}>{t("playlists.cancel")}</Button>
                <Button type="primary" onClick={confirmDownload}>
                  {t("playlists.ok")}
                </Button>
              </>
            )}
            </Space>
          </div>
        }
      >
        <Space direction="vertical" style={{ width: "100%" }} size="middle">
          {dlPaused && <Alert type="warning" showIcon message={t("app.syncPaused")} />}
          <Space.Compact style={{ width: "100%" }}>
            <Input
              placeholder={t("playlists.dlDirPlaceholder")}
              value={dlOptions.targetDir ?? ""}
              readOnly
            />
            <Button icon={<FolderOpenOutlined />} onClick={pickDownloadDir}>
              {t("playlists.choose")}
            </Button>
          </Space.Compact>
          <Space direction="vertical" style={{ width: "100%" }} size={2}>
            <Input
              placeholder={t("playlists.dlNamePlaceholder")}
              value={dlOptions.filenameTemplate ?? ""}
              onChange={(e) => setDlOptions((o) => ({ ...o, filenameTemplate: e.target.value }))}
            />
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {t("playlists.dlNameHint", { vars: VARIABLE_HINT })}
            </Typography.Text>
          </Space>
          <Select
            style={{ width: "100%" }}
            placeholder={t("playlists.dlQualityPlaceholder")}
            allowClear
            options={QUALITY_OPTIONS.map((value) => ({
              value,
              label: t(`playlists.qualityOptions.${value}`),
            }))}
            value={dlOptions.quality ?? undefined}
            onChange={(value) => setDlOptions((o) => ({ ...o, quality: value }))}
          />
          <Checkbox
            checked={dlOptions.writeLrc ?? false}
            onChange={(e) => setDlOptions((o) => ({ ...o, writeLrc: e.target.checked }))}
          >
            {t("playlists.dlLyrics")}
          </Checkbox>
          <Checkbox
            checked={dlOptions.overwrite}
            onChange={(e) => setDlOptions((o) => ({ ...o, overwrite: e.target.checked }))}
          >
            {t("playlists.dlOverwrite")}
          </Checkbox>
          {dlFailures.length > 0 && !dlDownloading && (
            <Alert
              type="warning"
              showIcon
              message={t("playlists.failedCount", { count: dlFailures.length })}
              description={
                <Space direction="vertical" size={2} style={{ width: "100%" }}>
                  {dlFailures.slice(0, 5).map((f, idx) => (
                    <Typography.Text key={idx} style={{ fontSize: 12 }}>
                      {f.song.name}：{formatError(f.message)}
                    </Typography.Text>
                  ))}
                  {dlFailures.length > 5 && (
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      … 其余 {dlFailures.length - 5} 首
                    </Typography.Text>
                  )}
                  <Button size="small" onClick={retryFailedOnly}>
                    {t("playlists.retryFailed")}
                  </Button>
                </Space>
              }
            />
          )}
        </Space>
      </Modal>

      <Modal
        title={t("playlists.restorePreviewTitle")}
        open={restoreDiff !== null}
        onCancel={() => !restoreBusy && setRestoreDiff(null)}
        onOk={doRestore}
        okText={t("playlists.restoreSnapshot")}
        cancelText={t("playlists.cancel")}
        confirmLoading={restoreBusy}
      >
        {restoreDiff && (
          <Space direction="vertical" style={{ width: "100%" }} size="small">
            <Alert type="info" showIcon message={t("playlists.preRestoreNote")} />
            {restoreDiff.toAdd.length === 0 && restoreDiff.toRemove.length === 0 ? (
              <Typography.Text type="secondary">{t("playlists.restoreNoChange")}</Typography.Text>
            ) : (
              <>
                {restoreDiff.toAdd.length > 0 && (
                  <>
                    <Typography.Text strong>
                      {t("playlists.restoreAdd", { count: restoreDiff.toAdd.length })}
                    </Typography.Text>
                    <List
                      size="small"
                      bordered
                      style={{ maxHeight: 140, overflow: "auto" }}
                      dataSource={restoreDiff.toAdd}
                      renderItem={(v) => (
                        <List.Item style={{ padding: "2px 12px" }}>
                          <Typography.Text style={{ fontSize: 12 }}>{v.name ?? v.id}</Typography.Text>
                        </List.Item>
                      )}
                    />
                  </>
                )}
                {restoreDiff.toRemove.length > 0 && (
                  <>
                    <Typography.Text strong type="danger">
                      {t("playlists.restoreRemove", { count: restoreDiff.toRemove.length })}
                    </Typography.Text>
                    <List
                      size="small"
                      bordered
                      style={{ maxHeight: 140, overflow: "auto" }}
                      dataSource={restoreDiff.toRemove}
                      renderItem={(v) => (
                        <List.Item style={{ padding: "2px 12px" }}>
                          <Typography.Text style={{ fontSize: 12 }}>{v.name ?? v.id}</Typography.Text>
                        </List.Item>
                      )}
                    />
                  </>
                )}
              </>
            )}
          </Space>
        )}
      </Modal>

      {lastReport && lastReport.failed > 0 && (
        <Modal
          title={t("playlists.viewErrors")}
          open={!!lastReport}
          footer={<Button onClick={() => setLastReport(null)}>{t("playlists.cancel")}</Button>}
          onCancel={() => setLastReport(null)}
        >
          <List
            size="small"
            dataSource={reportErrorList}
            renderItem={(item) => (
              <List.Item>
                <Typography.Text style={{ fontSize: 13 }}>
                  {item.trackName}：{translateUi(item.message)}
                </Typography.Text>
              </List.Item>
            )}
          />
        </Modal>
      )}
    </div>
  );
}
