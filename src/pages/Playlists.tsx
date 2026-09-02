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
import { api } from "../api";
import type {
  LoginStatus,
  PlaylistInfo,
  PlaylistSong,
  PlaylistSongsResult,
  SingleDownloadOptions,
} from "../types";
import type { SyncEventState } from "../App";

interface Props {
  login: LoginStatus | null;
  sync: SyncEventState;
}

const QUALITY_OPTIONS = [
  { value: "standard", label: "标准" },
  { value: "higher", label: "较高" },
  { value: "exhigh", label: "极高 320k" },
  { value: "lossless", label: "无损 FLAC" },
  { value: "hires", label: "Hi-Res" },
];

function formatDuration(millis: number): string {
  if (!millis) return "-";
  const total = Math.round(millis / 1000);
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

export default function PlaylistsPage({ login, sync }: Props) {
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
      antMessage.error(`加载歌单失败：${e}`);
    } finally {
      setLoading(false);
    }
  }, [login?.loggedIn]);

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
      antMessage.error(`加载歌曲列表失败：${e}`);
      setDetailId(null);
    } finally {
      setSongsLoading(false);
    }
  }, []);

  const openDownloadDialog = (playlistId: number, song: PlaylistSong) => {
    setDlTarget({ playlistId, song });
    setDlOptions({ overwrite: song.synced });
  };

  const pickDownloadDir = async () => {
    const path = (await open({ directory: true, multiple: false, title: "选择保存目录" })) as
      | string
      | null;
    if (path) setDlOptions((o) => ({ ...o, targetDir: path }));
  };

  const confirmDownload = async () => {
    if (!dlTarget) return;
    const { playlistId, song } = dlTarget;
    setDlDownloading(true);
    try {
      const path = await api.downloadSongWithOptions(playlistId, song.id, dlOptions);
      antMessage.success(`已下载：${song.name} → ${path}`);
      setDlTarget(null);
      setSongs(await api.getPlaylistSongs(playlistId));
      load();
    } catch (e) {
      antMessage.error(`下载失败：${e}`);
    } finally {
      setDlDownloading(false);
    }
  };

  if (!login?.loggedIn) {
    return (
      <div style={{ padding: 24 }}>
        <Alert message="请先在「账号登录」页扫码登录，再管理歌单同步。" type="warning" showIcon />
      </div>
    );
  }

  const shown = playlists.filter((p) =>
    p.name.toLowerCase().includes(filter.toLowerCase())
  );

  const currentPlaylist = playlists.find((p) => p.id === detailId);

  const columns: ColumnsType<PlaylistSong> = [
    {
      title: "#",
      dataIndex: "position",
      width: 48,
      render: (value: number) => <Typography.Text type="secondary">{value}</Typography.Text>,
    },
    { title: "歌曲", dataIndex: "name", ellipsis: true },
    { title: "歌手", dataIndex: "artists", ellipsis: true },
    { title: "专辑", dataIndex: "album", ellipsis: true },
    {
      title: "时长",
      dataIndex: "durationMs",
      width: 72,
      render: (value: number) => formatDuration(value),
    },
    {
      title: "状态",
      dataIndex: "synced",
      width: 84,
      render: (synced: boolean) =>
        synced ? <Tag color="success">已同步</Tag> : <Tag>未同步</Tag>,
    },
    {
      title: "操作",
      key: "action",
      width: 96,
      render: (_, song) => (
        <Button
          size="small"
          icon={<DownloadOutlined />}
          disabled={dlDownloading}
          onClick={() => detailId && openDownloadDialog(detailId, song)}
        >
          下载
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
              placeholder="搜索歌单"
              allowClear
              style={{ width: 240 }}
              onSearch={setFilter}
              onChange={(e) => !e.target.value && setFilter("")}
            />
            <Typography.Text type="secondary">
              共 {playlists.length} 个歌单，开启开关即可纳入自动同步
            </Typography.Text>
          </Space>
          <Space>
            <Button icon={<ReloadOutlined />} onClick={load} disabled={sync.running}>
              刷新
            </Button>
            <Button
              type="primary"
              icon={<SyncOutlined spin={sync.running} />}
              loading={sync.running}
              onClick={async () => {
                try {
                  await api.syncAll();
                  antMessage.success("全部歌单同步完成");
                  load();
                } catch (e) {
                  antMessage.error(`同步失败：${e}`);
                }
              }}
            >
              立即同步全部
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
            return (
              <List.Item
                actions={[
                  <Button
                    key="detail"
                    size="small"
                    icon={<EyeOutlined />}
                    onClick={() => openDetail(p.id)}
                  >
                    查看
                  </Button>,
                  <Switch
                    key="sw"
                    checked={p.enabled}
                    checkedChildren="同步"
                    unCheckedChildren="关闭"
                    onChange={async (v) => {
                      await api.setPlaylistEnabled(p.id, v);
                      load();
                    }}
                  />,
                  <Button
                    key="go"
                    size="small"
                    disabled={sync.running}
                    onClick={async () => {
                      try {
                        await api.syncPlaylist(p.id);
                        antMessage.success(`「${p.name}」同步完成`);
                        load();
                      } catch (e) {
                        antMessage.error(`同步失败：${e}`);
                      }
                    }}
                  >
                    立即同步
                  </Button>,
                ]}
              >
                <List.Item.Meta
                  avatar={<Avatar shape="square" size={48} src={p.coverImgUrl} />}
                  title={
                    <Space>
                      <span>{p.name}</span>
                      {p.subscribed && <Tag>收藏</Tag>}
                    </Space>
                  }
                  description={
                    <Space direction="vertical" size={2} style={{ width: "100%", maxWidth: 420 }}>
                      <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                        {p.trackCount} 首 · 已同步 {p.synced}/{p.trackCount}
                        {p.lastSync ? ` · 上次同步 ${p.lastSync}` : " · 从未同步"}
                        {p.lastResult ? ` · ${p.lastResult}` : ""}
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
        title={songs ? `${songs.playlistName} · ${songs.songs.length} 首` : "歌曲列表"}
        width={760}
        open={detailId !== null}
        onClose={() => setDetailId(null)}
        extra={
          <Space>
            <Space size={4}>
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                覆盖已存在文件
              </Typography.Text>
              <Switch
                size="small"
                checked={currentPlaylist?.overwrite ?? false}
                onChange={async (value) => {
                  if (!detailId) return;
                  await api.setPlaylistOverwrite(detailId, value);
                  load();
                }}
              />
            </Space>
            <Button
              type="primary"
              icon={<SyncOutlined />}
              disabled={sync.running}
              onClick={async () => {
                if (!detailId) return;
                try {
                  await api.syncPlaylist(detailId);
                  antMessage.success("同步完成，已刷新歌曲状态");
                  setSongs(await api.getPlaylistSongs(detailId));
                  load();
                } catch (e) {
                  antMessage.error(`同步失败：${e}`);
                }
              }}
            >
              同步缺失歌曲
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
          pagination={{ pageSize: 50, showSizeChanger: false }}
        />
      </Drawer>

      <Modal
        title={`下载歌曲：${dlTarget?.song.name ?? ""}`}
        open={dlTarget !== null}
        onCancel={() => !dlDownloading && setDlTarget(null)}
        onOk={confirmDownload}
        okText="下载"
        cancelText="取消"
        confirmLoading={dlDownloading}
      >
        <Space direction="vertical" style={{ width: "100%" }} size="middle">
          <Space.Compact style={{ width: "100%" }}>
            <Input
              placeholder="保存目录（留空使用歌单默认目录）"
              value={dlOptions.targetDir ?? ""}
              readOnly
            />
            <Button icon={<FolderOpenOutlined />} onClick={pickDownloadDir}>
              选择
            </Button>
          </Space.Compact>
          <Input
            placeholder="文件名模板（留空使用全局设置），变量：{音轨号} {歌手} {标题} {专辑} {网易云ID}"
            value={dlOptions.filenameTemplate ?? ""}
            onChange={(e) => setDlOptions((o) => ({ ...o, filenameTemplate: e.target.value }))}
          />
          <Select
            style={{ width: "100%" }}
            placeholder="音质（默认使用全局设置）"
            allowClear
            options={QUALITY_OPTIONS}
            value={dlOptions.quality ?? undefined}
            onChange={(value) => setDlOptions((o) => ({ ...o, quality: value }))}
          />
          <Checkbox
            checked={dlOptions.writeLrc ?? false}
            onChange={(e) => setDlOptions((o) => ({ ...o, writeLrc: e.target.checked }))}
          >
            同时下载歌词
          </Checkbox>
          <Checkbox
            checked={dlOptions.overwrite}
            onChange={(e) => setDlOptions((o) => ({ ...o, overwrite: e.target.checked }))}
          >
            覆盖已存在的同名文件
          </Checkbox>
        </Space>
      </Modal>
    </div>
  );
}