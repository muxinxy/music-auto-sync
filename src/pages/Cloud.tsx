import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { Key } from "react";
import {
  Alert,
  Button,
  Card,
  Input,
  Space,
  Statistic,
  Table,
  Tooltip,
  Typography,
  message as antMessage,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  CloudUploadOutlined,
  DownloadOutlined,
  ReloadOutlined,
  UploadOutlined,
} from "@ant-design/icons";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { formatError } from "../errors";
import { syncStore } from "../syncStore";
import { cloudStore } from "../cloudStore";
import type { CloudListResult, CloudSong, LoginStatus } from "../types";

const AUDIO_FILTERS = [
  { name: "Audio", extensions: ["mp3", "flac", "m4a", "wav", "ogg", "aac"] },
];

function formatSize(bytes?: number | null): string {
  if (!bytes || bytes <= 0) return "-";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

/** 云盘页：云盘歌曲列表；同步/手动上传/多选下载入口，任务进度在「同步任务」页查看。 */
export default function CloudPage({
  login,
  onGoSync,
  onGoSettings,
}: {
  login: LoginStatus | null;
  onGoSync: () => void;
  onGoSettings: () => void;
}) {
  const { t } = useTranslation();
  const [result, setResult] = useState<CloudListResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState("");
  const [selectedRows, setSelectedRows] = useState<CloudSong[]>([]);

  const running = useSyncExternalStore(syncStore.subscribeRunning, syncStore.getRunning);
  const cloudRunning = useSyncExternalStore(
    cloudStore.subscribeRunning,
    cloudStore.getRunning
  );

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setResult(await api.listCloudSongs());
    } catch (e) {
      antMessage.error(t("cloud.loadFailed", { detail: formatError(e) }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  // 后台刷新完成推送（重启后先展示磁盘缓存旧数据，随后由此事件更新为最新）。
  useEffect(() => {
    const unlisten = listen<CloudListResult>("cloud://list", (event) => {
      setResult(event.payload);
      setLoading(false);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // 云盘任务从运行转空闲时自动刷新列表（无论从本页还是别处发起/结束）。
  const prevRunning = useRef(running);
  useEffect(() => {
    if (prevRunning.current && !running) load();
    prevRunning.current = running;
  }, [running, load]);

  const startCloudAndGo = async () => {
    // 未设置音乐根目录时不发起任务：提示并直接跳设置页。
    try {
      const cfg = await api.getConfig();
      if (!cfg.musicRoot) {
        antMessage.warning(t("errors.musicRootRequired"));
        onGoSettings();
        return;
      }
    } catch {
      // 配置读取失败不拦截，交由后端报错兜底
    }
    // 不弹预览窗：直接开始任务并跳到「同步任务」页看实时进度/明细。
    api.syncCloudUpload().catch((e) => antMessage.error(formatError(e)));
    onGoSync();
  };

  /** 手动上传：选文件或目录 → 直接发起云盘任务并跳到「同步任务」页。 */
  const pickAndUpload = async (directory: boolean) => {
    let picked: string | string[] | null = null;
    if (directory) {
      picked = await openFileDialog({ directory: true, title: t("cloud.pickDir") });
    } else {
      picked = await openFileDialog({
        multiple: true,
        filters: AUDIO_FILTERS,
        title: t("cloud.pickFiles"),
      });
    }
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (paths.length === 0) return;
    api.syncCloudManual(paths).catch((e) => antMessage.error(formatError(e)));
    onGoSync();
  };

  /** 多选下载：选择目标目录后发起后台下载任务（进度/暂停/详情在「同步任务」页）。 */
  const startDownload = async () => {
    if (selectedRows.length === 0) return;
    let defaultDir: string | undefined;
    try {
      defaultDir = (await api.getConfig()).musicRoot ?? undefined;
    } catch {
      // 读取失败则不预填默认目录
    }
    const picked = await openFileDialog({
      directory: true,
      title: t("cloud.dlPickDir"),
      defaultPath: defaultDir,
    });
    if (!picked) return;
    api
      .downloadCloudSongs(
        selectedRows.map((s) => ({ id: s.songId, fileName: s.fileName })),
        picked as string
      )
      .catch((e) => antMessage.error(formatError(e)));
    onGoSync();
  };

  const songs = result?.songs ?? [];
  const filtered = useMemo(() => {
    const kw = search.trim().toLowerCase();
    if (!kw) return songs;
    return songs.filter((s) =>
      `${s.songName} ${s.artist} ${s.album} ${s.fileName}`.toLowerCase().includes(kw)
    );
  }, [songs, search]);

  const localTotalSize = useMemo(
    () => songs.reduce((sum, s) => sum + (s.fileSize || 0), 0),
    [songs]
  );

  if (!login?.loggedIn) {
    return (
      <div style={{ padding: 24 }}>
        <Alert type="warning" showIcon message={t("cloud.needLogin")} />
      </div>
    );
  }

  const columns: ColumnsType<CloudSong> = [
    { title: t("cloud.colSong"), dataIndex: "songName", ellipsis: true },
    { title: t("cloud.colArtist"), dataIndex: "artist", width: 140, ellipsis: true },
    { title: t("cloud.colAlbum"), dataIndex: "album", width: 160, ellipsis: true },
    {
      title: t("cloud.colSize"),
      dataIndex: "fileSize",
      width: 110,
      render: (v: number, s) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {formatSize(v)}
          {s.bitrate ? ` · ${s.bitrate}k` : ""}
        </Typography.Text>
      ),
    },
    { title: t("cloud.colFile"), dataIndex: "fileName", ellipsis: true },
    {
      title: t("cloud.colAdded"),
      dataIndex: "addTime",
      width: 150,
      render: (v?: number | null) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {v ? new Date(v).toLocaleString() : "-"}
        </Typography.Text>
      ),
    },
  ];

  const rowKeyOf = (s: CloudSong) => `${s.songId}-${s.fileName}`;

  return (
    <div style={{ padding: 24 }}>
      <Card
        title={t("cloud.title")}
        styles={{ body: { padding: 0 } }}
        extra={
          <Space wrap>
            <Button icon={<ReloadOutlined />} onClick={load} loading={loading}>
              {t("cloud.refresh")}
            </Button>
            <Tooltip title={t("cloud.syncBtnTip")}>
              <Button
                type="primary"
                icon={<CloudUploadOutlined />}
                onClick={startCloudAndGo}
                disabled={cloudRunning}
              >
                {t("cloud.syncBtn")}
              </Button>
            </Tooltip>
            <Tooltip title={t("cloud.uploadPickTip")}>
              <Button
                icon={<UploadOutlined />}
                disabled={cloudRunning}
                onClick={() => pickAndUpload(false)}
              >
                {t("cloud.uploadFiles")}
              </Button>
            </Tooltip>
            <Tooltip title={t("cloud.uploadPickTip")}>
              <Button
                icon={<UploadOutlined />}
                disabled={cloudRunning}
                onClick={() => pickAndUpload(true)}
              >
                {t("cloud.uploadDir")}
              </Button>
            </Tooltip>
            <Button
              icon={<DownloadOutlined />}
              disabled={selectedRows.length === 0 || cloudRunning}
              onClick={startDownload}
            >
              {t("cloud.downloadSelected", { count: selectedRows.length })}
            </Button>
          </Space>
        }
      >
        <div style={{ padding: 16, paddingBottom: 4 }}>
          <Space size={24} wrap align="center" style={{ marginBottom: 12 }}>
            <Statistic
              title={t("cloud.statsCount")}
              value={result?.storage.count ?? songs.length}
            />
            <Statistic
              title={t("cloud.statsSize")}
              value={formatSize(result?.storage.usedSize ?? localTotalSize)}
            />
            {result?.storage.maxSize ? (
              <Statistic
                title={t("cloud.statsQuota")}
                value={formatSize(result.storage.maxSize)}
              />
            ) : null}
          </Space>
          <Input.Search
            placeholder={t("cloud.searchPlaceholder")}
            allowClear
            style={{ width: 280, display: "block" }}
            onSearch={setSearch}
            onChange={(e) => !e.target.value && setSearch("")}
          />
        </div>
        <Table<CloudSong>
          size="small"
          rowKey={rowKeyOf}
          loading={loading}
          dataSource={filtered}
          columns={columns}
          rowSelection={{
            selectedRowKeys: selectedRows.map(rowKeyOf),
            onChange: (keys: Key[], rows) => setSelectedRows(rows),
            preserveSelectedRowKeys: true,
            selections: true,
          }}
          pagination={{
            pageSize: 50,
            showSizeChanger: false,
            showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}`,
          }}
          locale={{ emptyText: t("cloud.empty") }}
        />
      </Card>
    </div>
  );
}
