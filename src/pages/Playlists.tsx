import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Avatar,
  Button,
  Card,
  Checkbox,
  Drawer,
  Input,
  List,
  Modal,
  Progress,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
  message as antMessage,
} from "antd";
import {
  DownloadOutlined,
  EyeOutlined,
  FolderOpenOutlined,
  ReloadOutlined,
  SyncOutlined,
} from "@ant-design/icons";
import type { ColumnsType } from "antd/es/table";
import { open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { formatError, translateUi, uiMessage } from "../errors";
import type {
  LoginStatus,
  PlaylistInfo,
  PlaylistSong,
  PlaylistSongsResult,
  SingleDownloadOptions,
  UiMessage,
} from "../types";
import type { SyncEventState } from "../App";

interface Props {
  login: LoginStatus | null;
  sync: SyncEventState;
}

const QUALITY_OPTIONS = ["standard", "higher", "exhigh", "lossless", "hires"] as const;
const VARIABLE_HINT = "{音轨号} {歌手} {标题} {专辑} {网易云ID}";

function formatDuration(millis: number): string {
  if (!millis) return "-";
  const total = Math.round(millis / 1000);
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
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

export default function PlaylistsPage({ login, sync }: Props) {
  const { t } = useTranslation();
  const [playlists, setPlaylists] = useState<PlaylistInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState("");
  const [detailId, setDetailId] = useState<number | null>(null);
  const [songs, setSongs] = useState<PlaylistSongsResult | null>(null);
  const [songsLoading, setSongsLoading] = useState(false);

  const [dlTarget, setDlTarget] = useState<{ playlistId: number; song: PlaylistSong } | null>(null);
  const [dlOptions, setDlOptions] = useState<SingleDownloadOptions>({ overwrite: false });
  const [dlDownloading, setDlDownloading] = useState(false);

  const load = useCallback(async () => {
    if (!login?.loggedIn) return;
    setLoading(true);
    try {
      setPlaylists(await api.listPlaylists());
    } catch (e) {
      antMessage.error(t("playlists.loadFailed", { detail: formatError(e) }));
    } finally {
      setLoading(false);
    }
  }, [login?.loggedIn, t]);

  useEffect(() => {
    load();
  }, [load]);

  const openDetail = useCallback(async (id: number) => {
    setDetailId(id);
    setSongsLoading(true);
    setSongs(null);
    try {
      setSongs(await api.getPlaylistSongs(id));
    } catch (e) {
      antMessage.error(t("playlists.loadSongsFailed", { detail: formatError(e) }));
      setDetailId(null);
    } finally {
      setSongsLoading(false);
    }
  }, [t]);

  const openDownloadDialog = (playlistId: number, song: PlaylistSong) => {
    setDlTarget({ playlistId, song });
    setDlOptions({ overwrite: song.synced, writeLrc: false });
  };

  const pickDownloadDir = async () => {
    const path = (await open({ directory: true, multiple: false, title: t("playlists.choose") })) as
      | string
      | null;
    if (path) setDlOptions((o) => ({ ...o, targetDir: path }));
  };

  const confirmDownload = async () => {
    if (!dlTarget) return;
    const { playlistId, song } = dlTarget;
    setDlDownloading(true);
    try {
      const path = await api.downloadSongWithOptions(playlistId, song.id, {
        ...dlOptions,
        writeLrc: dlOptions.writeLrc ?? false,
      });
      antMessage.success(t("playlists.downloaded", { name: song.name }));
      setDlTarget(null);
      setSongs(await api.getPlaylistSongs(playlistId));
      load();
    } catch (e) {
      const ui = uiMessage(e);
      antMessage.error(
        ui.code === "fileExists"
          ? translateUi(ui)
          : t("playlists.downloadFailed", { detail: formatError(e) })
      );
    } finally {
      setDlDownloading(false);
    }
  };

  const runSync = async (id: number, messageName: string) => {
    try {
      await api.syncPlaylist(id);
      antMessage.success(t("playlists.playlistSynced", { name: messageName }));
      setSongs(await api.getPlaylistSongs(id));
      load();
    } catch (e) {
      antMessage.error(t("playlists.syncFailed", { detail: formatError(e) }));
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

    const columns: ColumnsType<PlaylistSong> = [
    {
      title: t("playlists.columns.no"),
      dataIndex: "position",
      width: 48,
      render: (value: number) => <Typography.Text type="secondary">{value}</Typography.Text>,
    },
    { title: t("playlists.columns.song"), dataIndex: "name", ellipsis: true },
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
      width: 84,
      render: (synced: boolean) =>
        synced ? <Tag color="success">{t("playlists.syncedTag")}</Tag> : <Tag>{t("playlists.unsyncedTag")}</Tag>,
    },
    {
      title: t("playlists.columns.action"),
      key: "action",
      width: 96,
      render: (_, song) => (
        <Button
          size="small"
          icon={<DownloadOutlined />}
          disabled={dlDownloading}
          onClick={() => detailId && openDownloadDialog(detailId, song)}
        >
          {t("playlists.download")}
        </Button>
      ),
    },
  ];

  return (
    <div style={{ padding: 24 }}>
      <Card style={{ marginBottom: 16 }}>
        <Space style={{ width: "100%", justifyContent: "space-between" }}>
          <Space>
            <Input.Search
              placeholder={t("playlists.searchPlaceholder")}
              allowClear
              style={{ width: 240 }}
              onSearch={setFilter}
              onChange={(e) => !e.target.value && setFilter("")}
            />
            <Typography.Text type="secondary">
              {t("playlists.countHint", { count: playlists.length })}
            </Typography.Text>
          </Space>
          <Space>
            <Button icon={<ReloadOutlined />} onClick={load} disabled={sync.running}>
              {t("playlists.refresh")}
            </Button>
            <Button
              type="primary"
              icon={<SyncOutlined spin={sync.running} />}
              loading={sync.running}
              onClick={async () => {
                try {
                  await api.syncAll();
                  antMessage.success(t("playlists.allSynced"));
                  load();
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
                      load();
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
                      load();
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
        width={760}
        open={detailId !== null}
        onClose={() => setDetailId(null)}
        extra={
          <Button
            type="primary"
            icon={<SyncOutlined />}
            disabled={sync.running}
            onClick={() => detailId && runSync(detailId, songs?.playlistName ?? "")}
          >
            {t("playlists.syncMissing")}
          </Button>
        }
      >
        <Table<PlaylistSong>
          rowKey="id"
          size="small"
          loading={songsLoading}
          dataSource={songs?.songs ?? []}
          columns={columns}
          pagination={{ pageSize: 50, showSizeChanger: false }}
        />
      </Drawer>

      <Modal
        title={t("playlists.downloadTitle", { name: dlTarget?.song.name ?? "" })}
        open={dlTarget !== null}
        onCancel={() => !dlDownloading && setDlTarget(null)}
        onOk={confirmDownload}
        okText={t("playlists.ok")}
        cancelText={t("playlists.cancel")}
        confirmLoading={dlDownloading}
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
        </Space>
      </Modal>
    </div>
  );
}