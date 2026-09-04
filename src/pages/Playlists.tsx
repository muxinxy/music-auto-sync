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
  Select,
  Space,
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
  LoginStatus,
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
const playlistCache: { at: number; data: PlaylistInfo[] } = { at: 0, data: [] };

export default function PlaylistsPage({ login, sync }: Props) {
  const { t } = useTranslation();
  const [playlists, setPlaylists] = useState<PlaylistInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState("");
  const [detailId, setDetailId] = useState<number | null>(null);
  const [songs, setSongs] = useState<PlaylistSongsResult | null>(null);
  const [songsLoading, setSongsLoading] = useState(false);
  const [availability, setAvailability] = useState<Record<number, TrackAvailability>>({});
  const [availabilityLoading, setAvailabilityLoading] = useState(false);

  const [selectedPlaylists, setSelectedPlaylists] = useState<Set<number>>(new Set());
  const [selectedSongs, setSelectedSongs] = useState<number[]>([]);

  const [dlTarget, setDlTarget] = useState<{
    playlistId: number;
    songs: PlaylistSong[];
  } | null>(null);
  const [dlOptions, setDlOptions] = useState<SingleDownloadOptions>({ overwrite: false });
  const [dlDownloading, setDlDownloading] = useState(false);
  const [dlFailures, setDlFailures] = useState<{ song: PlaylistSong; message: UiMessage }[]>([]);
  const [lastReport, setLastReport] = useState<SyncReport | null>(null);

  const load = useCallback(
    async (force = false) => {
      if (!login?.loggedIn) return;
      if (!force && Date.now() - playlistCache.at < CACHE_TTL_MS && playlistCache.data.length > 0) {
        setPlaylists(playlistCache.data);
        return;
      }
      setLoading(true);
      try {
        const data = await api.listPlaylists();
        playlistCache.at = Date.now();
        playlistCache.data = data;
        setPlaylists(data);
      } catch (e) {
        antMessage.error(t("playlists.loadFailed", { detail: formatError(e) }));
      } finally {
        setLoading(false);
      }
    },
    [login?.loggedIn, t]
  );

  useEffect(() => {
    load();
  }, [load]);

  const openDetail = useCallback(
    async (id: number) => {
      setDetailId(id);
      setSongsLoading(true);
      setSongs(null);
      setAvailability({});
      setLastReport(null);
      try {
        const result = await api.getPlaylistSongs(id);
        setSongs(result);
        // 后台预检可用性/最高音质（失败静默）。
        setAvailabilityLoading(true);
        api
          .preflightPlaylist(id)
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
    },
    [t]
  );

  const openDownloadDialog = (playlistId: number, songs: PlaylistSong[]) => {
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
    const failures: { song: PlaylistSong; message: UiMessage }[] = [];
    try {
      for (const song of targets) {
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
      if (targets.length === 1) {
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
      if (failures.length === 0) {
        setDlTarget(null);
        setSelectedSongs([]);
      }
      setSongs(await api.getPlaylistSongs(playlistId));
      load(true);
    } finally {
      setDlDownloading(false);
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

  const runSync = async (id: number, messageName: string) => {
    try {
      const report = await api.syncPlaylist(id);
      antMessage.success(t("playlists.playlistSynced", { name: messageName }));
      if (report.failed > 0) {
        setLastReport(report);
        antMessage.warning(t("playlists.syncDoneWithErrors", { failed: report.failed }));
      }
      setSongs(await api.getPlaylistSongs(id));
      load(true);
    } catch (e) {
      antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
    }
  };

  const prunePlaylist = async (id: number) => {
    try {
      const count = await api.manualPrune(id);
      if (count > 0) antMessage.success(t("playlists.pruneDone", { count }));
      else antMessage.info(t("quarantine.empty"));
      setSongs(await api.getPlaylistSongs(id));
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

  const shown = playlists.filter((p) =>
    p.name.toLowerCase().includes(filter.toLowerCase())
  );

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
        <Space size={4} style={{ flexWrap: "wrap" }}>
          <span>{value}</span>
          {reasonTag(song)}
        </Space>
      ),
    },
    { title: t("playlists.columns.artists"), dataIndex: "artists", ellipsis: true },
    { title: t("playlists.columns.album"), dataIndex: "album", ellipsis: true },
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
            <Typography.Text type="secondary">
              {t("playlists.countHint", { count: playlists.length })}
            </Typography.Text>
          </Space>
          <Space>
            <Button icon={<ReloadOutlined />} onClick={() => load(true)} disabled={sync.running}>
              {t("playlists.refresh")}
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
                  <Popconfirm
                    key="prune"
                    title={t("playlists.pruneConfirmTitle")}
                    description={t("playlists.pruneConfirmDesc")}
                    okText={t("playlists.ok")}
                    cancelText={t("playlists.cancel")}
                    onConfirm={() => prunePlaylist(p.id)}
                  >
                    <Button size="small" icon={<FolderOpenOutlined />}>
                      {t("playlists.prune")}
                    </Button>
                  </Popconfirm>,
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
            <Button
              icon={<DownloadOutlined />}
              disabled={selectedSongs.length === 0 || dlDownloading}
              onClick={openBatchDownloadSongs}
            >
              {t("playlists.downloadBatch", { count: selectedSongs.length })}
            </Button>
            <Button
              icon={<CloudDownloadOutlined />}
              disabled={dlDownloading}
              onClick={selectedPlaylistSongsAll}
            >
              {t("playlists.downloadMissing")}
            </Button>
            <Button
              type="primary"
              icon={<SyncOutlined />}
              disabled={sync.running}
              onClick={() => detailId && runSync(detailId, songs?.playlistName ?? "")}
            >
              {t("playlists.syncMissing")}
            </Button>
          </Space>
        }
      >
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
        title={
          dlTarget && dlTarget.songs.length > 1
            ? t("playlists.downloadTitleBatch", { count: dlTarget.songs.length })
            : t("playlists.downloadTitle", { name: dlTarget?.songs[0]?.name ?? "" })
        }
        open={dlTarget !== null}
        onCancel={() => !dlDownloading && setDlTarget(null)}
        onOk={dlFailures.length > 0 ? retryFailedOnly : confirmDownload}
        okText={dlFailures.length > 0 ? t("playlists.retryFailed") : t("playlists.ok")}
        cancelText={t("playlists.cancel")}
        confirmLoading={dlDownloading}
        okButtonProps={{ disabled: dlDownloading }}
      >
        <Space direction="vertical" style={{ width: "100%" }} size="middle">
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
