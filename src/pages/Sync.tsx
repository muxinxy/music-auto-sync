import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import {
  Button,
  Card,
  DatePicker,
  Input,
  List,
  Modal,
  Popconfirm,
  Progress,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message as antMessage,
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { UndoOutlined } from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import dayjs, { type Dayjs } from "dayjs";
import i18n from "../i18n";
import { api } from "../api";
import { syncStore } from "../syncStore";
import { cloudStore } from "../cloudStore";
import { formatError, translateUi } from "../errors";
import { taskDisplayName } from "../taskName";
import type { CloudTaskRow, RunChangeEntry, SyncErrorDetail, SyncProgress, SyncReport, UiMessage } from "../types";

interface LogEntry {
  id: number;
  ts: string;
  playlistName: string;
  status: string;
  message: string;
}

function renderMessage(raw: string): string {
  return messageCache.get(raw) ?? (() => {
    let result: string;
    if (raw.startsWith("{")) {
      try {
        result = translateUi(JSON.parse(raw) as UiMessage);
      } catch {
        result = raw;
      }
    } else {
      result = raw;
    }
    if (messageCache.size > 2000) messageCache.clear(); // 防长会话膨胀
    messageCache.set(raw, result);
    return result;
  })();
}

/** 已翻译消息缓存：同一 JSON 字符串多次渲染只解析翻译一次。 */
const messageCache = new Map<string, string>();

function actionLabel(action: string): string {
  const map: Record<string, string> = {
    added_local: "syncPage.action.addedLocal",
    quarantined_local: "syncPage.action.quarantinedLocal",
    added_playlist: "syncPage.action.addedPlaylist",
    removed_from_playlist: "syncPage.action.removedFromPlaylist",
    failed: "syncPage.action.failed",
  };
  return map[action] ?? action;
}

export default function SyncPage() {
  const { t } = useTranslation();
  const [reports, setReports] = useState<SyncReport[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [logFilter, setLogFilter] = useState("");
  const [logRange, setLogRange] = useState<[Dayjs | null, Dayjs | null] | null>(null);
  const [cloudDetailOpen, setCloudDetailOpen] = useState(false);
  const [runDetail, setRunDetail] = useState<{ runId: number; name: string } | null>(null);

  const loadLogs = useCallback(async () => {
    try {
      setLogs(await api.getSyncLogs(1000));
    } catch (e) {
      antMessage.error(t("syncPage.loadLogsFailed", { detail: String(e) }));
    }
  }, [t]);

  useEffect(() => {
    loadLogs();
    const un2 = listen<SyncReport>("sync://report", (e) => {
      setReports((r) => [e.payload, ...r].slice(0, 20));
      loadLogs();
    });
    // 云盘任务报告（完成/取消/失败都会发）→ 刷新日志。
    const un3 = listen<SyncReport>("cloud://report", () => loadLogs());
    // 任务开始/结束（歌单与云盘各自独立）→ 延迟半秒刷新，
    // 等后端写完/更新完那条唯一的任务日志再拉取。
    const un4 = listen<boolean>("sync://state", () => setTimeout(loadLogs, 500));
    const un5 = listen<boolean>("cloud://state", () => setTimeout(loadLogs, 500));
    return () => {
      un2.then((f) => f());
      un3.then((f) => f());
      un4.then((f) => f());
      un5.then((f) => f());
    };
  }, [loadLogs]);

  const clearLogs = async () => {
    try {
      const count = await api.clearSyncHistory("logs");
      antMessage.success(t("syncPage.cleared", { count }));
      loadLogs();
    } catch (e) {
      antMessage.error(formatError(e));
    }
  };

  return (
    <div style={{ padding: 24 }}>
      <CurrentTaskCard
        onOpenCloudDetail={() => setCloudDetailOpen(true)}
        onOpenRunDetail={(runId, name) => setRunDetail({ runId, name })}
      />

      <Modal
        title={t("cloud.taskDetail")}
        open={cloudDetailOpen}
        footer={null}
        onCancel={() => setCloudDetailOpen(false)}
        width={880}
      >
        <CloudTaskDetailTable />
      </Modal>

      <Modal
        title={
          runDetail
            ? `${taskDisplayName(runDetail.name)} · ${t("cloud.taskDetail")}`
            : t("cloud.taskDetail")
        }
        open={runDetail !== null}
        footer={null}
        onCancel={() => setRunDetail(null)}
        width={880}
      >
        {runDetail && <RunChangesTable runId={runDetail.runId} />}
      </Modal>

      {reports.length > 0 && (
        <Card title={t("syncPage.recentResults")} style={{ marginBottom: 16 }} size="small">
          <List
            size="small"
            dataSource={reports}
            renderItem={(r) => {
              const details: SyncErrorDetail[] = (r.errorDetails ?? []).map((d) => ({
                ...d,
                message: d.message,
              }));
              const hasDetails = details.length > 0;
              return (
                <List.Item
                  actions={[
                    r.playlistName === "cloud" ? (
                      <Button
                        key="detail"
                        size="small"
                        type="link"
                        onClick={() => setCloudDetailOpen(true)}
                      >
                        {t("cloud.taskDetail")}
                      </Button>
                    ) : hasDetails ? (
                      <Button
                        key="view"
                        size="small"
                        type="link"
                        onClick={() => {
                          setExpanded(expanded === r.startedAt ? null : r.startedAt);
                        }}
                      >
                        {t("syncPage.viewDetails", { count: r.failed })}
                      </Button>
                    ) : null,
                  ]}
                >
                  <List.Item.Meta
                    title={
                      <Typography.Text>
                        {t("syncPage.resultLine", {
                          name: taskDisplayName(r.playlistName),
                          added: r.added,
                          converted: r.ncmConverted,
                          quarantined: r.quarantined,
                          failed: r.failed,
                        })}
                      </Typography.Text>
                    }
                    description={
                      expanded === r.startedAt && hasDetails ? (
                        <List
                          size="small"
                          dataSource={details}
                          renderItem={(d) => (
                            <List.Item style={{ border: "none", padding: "2px 0" }}>
                              <Typography.Text type="danger" style={{ fontSize: 12 }}>
                                {taskDisplayName(d.trackName)}：{translateUi(d.message)}
                              </Typography.Text>
                            </List.Item>
                          )}
                        />
                      ) : undefined
                    }
                  />
                  <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                    {r.finishedAt}
                  </Typography.Text>
                </List.Item>
              );
            }}
          />
        </Card>
      )}

      <Card
        title={t("syncPage.syncLogs")}
        size="small"
        styles={{ body: { padding: 0 } }}
        extra={
          logs.length > 0 ? (
            <Popconfirm
              title={t("syncPage.clearConfirm")}
              okText={t("syncPage.clearLogs")}
              cancelText={t("playlists.cancel")}
              onConfirm={clearLogs}
            >
              <Button size="small" type="text">
                {t("syncPage.clearLogs")}
              </Button>
            </Popconfirm>
          ) : undefined
        }
      >
        <div style={{ padding: 12 }}>
          <Space wrap>
            <Input.Search
              placeholder={t("syncPage.filterLog")}
              allowClear
              style={{ width: 240 }}
              onSearch={setLogFilter}
              onChange={(e) => !e.target.value && setLogFilter("")}
            />
            <DatePicker.RangePicker
              showTime={{ format: "HH:mm" }}
              format="YYYY-MM-DD HH:mm"
              value={logRange}
              onChange={(value) => setLogRange(value as [Dayjs | null, Dayjs | null] | null)}
              allowClear
              size="small"
            />
          </Space>
        </div>
        <List
          size="small"
          dataSource={logs.filter((l) => {
            if (logRange && logRange[0] && logRange[1]) {
              const ts = dayjs(l.ts);
              if (ts.isBefore(logRange[0].startOf("second")) || ts.isAfter(logRange[1].endOf("second"))) {
                return false;
              }
            }
            if (logFilter) {
              const kw = logFilter.toLowerCase();
              // 任务名用翻译后的名称参与搜索，使“云盘”能命中哨兵名 "cloud" 的日志。
              const hay = `${taskDisplayName(l.playlistName)} ${renderMessage(l.message)}`.toLowerCase();
              return hay.includes(kw);
            }
            return true;
          })}
          pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: [20, 50, 100], showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}` }}
          locale={{ emptyText: t("syncPage.noLogs") }}
          renderItem={(l) => {
            const tag =
              l.status === "ok" ? (
                <Tag color="success">{t("syncPage.statusSuccess")}</Tag>
              ) : l.status === "error" ? (
                <Tag color="error">{t("syncPage.statusFailed")}</Tag>
              ) : l.status === "canceled" ? (
                <Tag color="warning">{t("syncPage.statusCanceled")}</Tag>
              ) : (
                <Tag color="processing">{t("syncPage.statusRunning")}</Tag>
              );
            return (
              <List.Item
                style={{ paddingLeft: 24, paddingRight: 24 }}
                actions={[
                  <Button
                    key="detail"
                    size="small"
                    type="link"
                    onClick={() => setRunDetail({ runId: l.id, name: l.playlistName })}
                  >
                    {t("cloud.taskDetail")}
                  </Button>,
                ]}
              >
                <List.Item.Meta
                  title={
                    <Typography.Text style={{ fontSize: 13 }}>
                      {tag} {taskDisplayName(l.playlistName) || "-"}
                    </Typography.Text>
                  }
                  description={renderMessage(l.message)}
                />
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {l.ts}
                </Typography.Text>
              </List.Item>
            );
          }}
        />
      </Card>
    </div>
  );
}

function formatSize(bytes?: number | null): string {
  if (!bytes || bytes <= 0) return "-";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

// ---------- 三态排序（递增 → 递减 → 默认，点击表头切换） ----------

type TriSort = { key: string; dir: "asc" | "desc" } | null;

function useTriSort() {
  const [sort, setSort] = useState<TriSort>(null);
  const toggle = (key: string) =>
    setSort((current) =>
      current?.key !== key
        ? { key, dir: "asc" }
        : current.dir === "asc"
          ? { key, dir: "desc" }
          : null
    );
  const arrow = (key: string) =>
    sort?.key === key ? (sort.dir === "asc" ? "↑" : "↓") : "⇅";
  return { sort, toggle, arrow };
}

type TriSortApi = ReturnType<typeof useTriSort>;

/** 可排序表头：字段名后带方向箭头，点击切换递增/递减/默认。 */
function SortHeader({
  label,
  field,
  tri,
}: {
  label: string;
  field: string;
  tri: TriSortApi;
}) {
  const active = tri.sort?.key === field;
  return (
    <span
      style={{ cursor: "pointer", userSelect: "none" }}
      onClick={() => tri.toggle(field)}
    >
      {label}
      <Typography.Text
        type={active ? undefined : "secondary"}
        style={{ fontSize: 11, marginLeft: 3 }}
      >
        {tri.arrow(field)}
      </Typography.Text>
    </span>
  );
}

function applyTriSort<T>(rows: T[], sort: TriSort, value: (r: T) => string | number): T[] {
  if (!sort) return rows;
  const sorted = [...rows].sort((a, b) => {
    const va = value(a);
    const vb = value(b);
    const cmp =
      typeof va === "number" && typeof vb === "number"
        ? va - vb
        : String(va).localeCompare(String(vb), "zh-CN");
    return cmp;
  });
  return sort.dir === "desc" ? sorted.reverse() : sorted;
}

const cloudResultMeta: Record<CloudTaskRow["result"], { labelKey: string; color: string }> = {
  in_cloud: { labelKey: "cloud.resultInCloud", color: "default" },
  duplicate: { labelKey: "cloud.resultDuplicate", color: "default" },
  unresolved: { labelKey: "cloud.resultUnresolved", color: "orange" },
  to_upload: { labelKey: "cloud.resultToUpload", color: "processing" },
  uploading: { labelKey: "cloud.resultUploading", color: "processing" },
  uploaded: { labelKey: "cloud.resultUploaded", color: "green" },
  instant: { labelKey: "cloud.resultInstant", color: "cyan" },
  downloading: { labelKey: "cloud.resultDownloading", color: "processing" },
  downloaded: { labelKey: "cloud.resultDownloaded", color: "green" },
  dl_skipped: { labelKey: "cloud.resultDlSkipped", color: "default" },
  failed: { labelKey: "cloud.resultFailed", color: "red" },
};

/** 单轮明细表：筛选（文件名/状态）+ 三态排序 + 汇总行 + 分页表格（每页 20，可切 50/100）。 */
function CloudTaskRowsTable({ rows }: { rows: CloudTaskRow[] }) {
  const { t } = useTranslation();
  const [keyword, setKeyword] = useState("");
  const [statusFilter, setStatusFilter] = useState("");
  const tri = useTriSort();
  // 状态筛选按本次任务实际出现的状态动态生成（上传/下载任务各自只列相关项）。
  const statusOptions = useMemo(() => {
    const present = new Set(rows.map((r) => r.result));
    return Object.entries(cloudResultMeta)
      .filter(([value]) => present.has(value as CloudTaskRow["result"]))
      .map(([value, meta]) => ({ value, label: t(meta.labelKey) }));
  }, [rows, t]);
  const counts = useMemo(() => {
    let upload = 0;
    let inCloud = 0;
    let duplicate = 0;
    let unresolved = 0;
    let failed = 0;
    for (const r of rows) {
      if (r.result === "to_upload" || r.result === "uploading") upload++;
      else if (r.result === "in_cloud") inCloud++;
      else if (r.result === "duplicate") duplicate++;
      else if (r.result === "unresolved") unresolved++;
      else if (r.result === "failed") failed++;
    }
    return { total: rows.length, upload, inCloud, duplicate, unresolved, failed };
  }, [rows]);

  const filtered = useMemo(() => {
    const kw = keyword.trim().toLowerCase();
    if (!kw && !statusFilter) return rows;
    return rows.filter((r) => {
      if (statusFilter && r.result !== statusFilter) return false;
      if (
        kw &&
        !`${r.fileName} ${r.title} ${r.artist} ${r.path}`.toLowerCase().includes(kw)
      ) {
        return false;
      }
      return true;
    });
  }, [rows, keyword, statusFilter]);

  const sorted = useMemo(
    () =>
      applyTriSort(filtered, tri.sort, (r) => {
        switch (tri.sort?.key) {
          case "fileName":
            return r.fileName;
          case "matched":
            return `${r.title}${r.artist}`;
          case "fileSize":
            return r.fileSize;
          case "result":
            return r.result;
          default:
            return "";
        }
      }),
    [filtered, tri.sort]
  );

  const columns: ColumnsType<CloudTaskRow> = [
    {
      title: <SortHeader label={t("cloud.detailColFile")} field="fileName" tri={tri} />,
      dataIndex: "fileName",
      ellipsis: true,
      render: (v: string, r) => <Typography.Text ellipsis={{ tooltip: r.path }}>{v}</Typography.Text>,
    },
    {
      title: <SortHeader label={t("cloud.detailColMatched")} field="matched" tri={tri} />,
      ellipsis: true,
      render: (_, r) =>
        r.title || r.artist
          ? `${r.title || r.fileName}${r.artist ? ` - ${r.artist}` : ""}`
          : r.neteaseId
            ? `id:${r.neteaseId}`
            : "-",
    },
    {
      title: <SortHeader label={t("cloud.detailColSize")} field="fileSize" tri={tri} />,
      dataIndex: "fileSize",
      width: 90,
      render: (v: number) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {formatSize(v)}
        </Typography.Text>
      ),
    },
    {
      title: <SortHeader label={t("cloud.detailColResult")} field="result" tri={tri} />,
      dataIndex: "result",
      width: 110,
      render: (result: CloudTaskRow["result"]) => {
        const meta = cloudResultMeta[result] ?? { labelKey: "", color: "default" };
        return <Tag color={meta.color}>{t(meta.labelKey)}</Tag>;
      },
    },
    {
      title: t("cloud.detailColNote"),
      dataIndex: "message",
      ellipsis: true,
      render: (m?: UiMessage | null) =>
        m ? (
          <Typography.Text type="danger" style={{ fontSize: 12 }}>
            {translateUi(m)}
          </Typography.Text>
        ) : (
          "-"
        ),
    },
  ];

  if (rows.length === 0) {
    return <Typography.Text type="secondary">{t("cloud.detailEmpty")}</Typography.Text>;
  }

  return (
    <>
      <Space style={{ marginBottom: 12 }} wrap>
        <Input.Search
          placeholder={t("cloud.filterFile")}
          allowClear
          style={{ width: 220 }}
          onSearch={setKeyword}
          onChange={(e) => !e.target.value && setKeyword("")}
        />
        <Select
          style={{ width: 150 }}
          placeholder={t("cloud.filterStatus")}
          allowClear
          value={statusFilter || undefined}
          onChange={(v) => setStatusFilter(v ?? "")}
          options={statusOptions}
        />
      </Space>
      <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
        {t("cloud.detailSummary", counts)}
      </Typography.Paragraph>
      <Table<CloudTaskRow>
        size="small"
        rowKey="path"
        dataSource={sorted}
        columns={columns}
        pagination={{
          pageSize: 20,
          showSizeChanger: true,
          pageSizeOptions: [20, 50, 100],
          showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}`,
        }}
      />
    </>
  );
}

/** 任务详情弹窗内容：本次任务的扫描/上传明细（实时）。 */
function CloudTaskDetailTable() {
  const { t } = useTranslation();
  const rows = useSyncExternalStore(cloudStore.subscribeRows, cloudStore.getRows);
  if (rows.length === 0) {
    return <Typography.Text type="secondary">{t("cloud.detailEmpty")}</Typography.Text>;
  }
  return <CloudTaskRowsTable rows={rows} />;
}

/**
 * “当前任务”卡片：云盘任务与歌单任务**并行**运行，各自独立显示进度与操作按钮
 * （「任务详情」仅云盘任务有；暂停/继续/取消按任务独立生效）。
 * 独立订阅高频进度 store，使每曲目进度更新只重渲染本卡片，不波及下方大列表。
 */
function CurrentTaskCard({
  onOpenCloudDetail,
  onOpenRunDetail,
}: {
  onOpenCloudDetail: () => void;
  onOpenRunDetail: (runId: number, name: string) => void;
}) {
  const { t } = useTranslation();
  const progress = useSyncExternalStore(syncStore.subscribeProgress, syncStore.getProgress);
  const running = useSyncExternalStore(syncStore.subscribeRunning, syncStore.getRunning);
  const paused = useSyncExternalStore(syncStore.subscribeRunning, syncStore.getPaused);
  const cloudProgress = useSyncExternalStore(cloudStore.subscribeProgress, cloudStore.getProgress);
  const cloudRunning = useSyncExternalStore(cloudStore.subscribeRunning, cloudStore.getRunning);
  const cloudPaused = useSyncExternalStore(cloudStore.subscribeRunning, cloudStore.getPaused);

  const cancelPop = (onConfirm: () => void) => (
    <Popconfirm
      title={t("app.cancelConfirm")}
      okText={t("app.cancel")}
      cancelText={t("playlists.cancel")}
      onConfirm={onConfirm}
    >
      <Button size="small" danger>
        {t("app.cancelTask")}
      </Button>
    </Popconfirm>
  );

  const cloudActive = cloudRunning && cloudProgress;
  const playlistActive = running && progress;

  return (
    <Card title={t("syncPage.currentTask")} size="small" style={{ marginBottom: 16 }}>
      {cloudActive || playlistActive ? (
        <Space direction="vertical" style={{ width: "100%" }} size={16}>
          {cloudActive && (
            <TaskProgressRow progress={cloudProgress} paused={cloudPaused}>
              <Button size="small" onClick={onOpenCloudDetail}>
                {t("cloud.taskDetail")}
              </Button>
              {cloudPaused ? (
                <Button size="small" type="primary" onClick={() => api.resumeCloudSync()}>
                  {t("app.resume")}
                </Button>
              ) : (
                <Button size="small" onClick={() => api.pauseCloudSync()}>
                  {t("app.pause")}
                </Button>
              )}
              {cancelPop(() => api.cancelCloudSync())}
            </TaskProgressRow>
          )}
          {playlistActive && (
            <TaskProgressRow progress={progress} paused={paused}>
              {progress.runId != null && (
                <Button
                  size="small"
                  onClick={() => onOpenRunDetail(progress.runId as number, progress.playlistName)}
                >
                  {t("cloud.taskDetail")}
                </Button>
              )}
              {paused ? (
                <Button size="small" type="primary" onClick={() => api.resumeSync()}>
                  {t("app.resume")}
                </Button>
              ) : (
                <Button size="small" onClick={() => api.pauseSync()}>
                  {t("app.pause")}
                </Button>
              )}
              {cancelPop(() => api.cancelSync())}
            </TaskProgressRow>
          )}
        </Space>
      ) : (
        <Typography.Text type="secondary">{t("syncPage.noTask")}</Typography.Text>
      )}
    </Card>
  );
}

/** 单个任务的进度行：阶段标签 + 进度条 + 行内操作按钮。 */
function TaskProgressRow({
  progress,
  paused,
  children,
}: {
  progress: SyncProgress;
  paused: boolean;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const phaseLabel = t(`phases.${progress.phase}`, { defaultValue: progress.phase });
  const progressMessage =
    progress.message.code === "track" && progress.message.params?.[0]
      ? progress.message.params[0]
      : translateUi(progress.message);
  return (
    <div>
      <Typography.Paragraph style={{ marginBottom: 4 }}>
        <Tag color="processing">{phaseLabel}</Tag>
        {taskDisplayName(progress.playlistName)} —— {progressMessage}
      </Typography.Paragraph>
      <Progress
        percent={progress.total ? Math.round((progress.current / progress.total) * 100) : 0}
        status={paused ? "normal" : "active"}
      />
      <Space style={{ marginTop: 6 }} wrap>
        {children}
      </Space>
    </div>
  );
}

function actionColor(action: string): string {
  if (action === "added_local" || action === "added_playlist" || action === "added_cloud") {
    return "green";
  }
  if (action === "quarantined_local" || action === "removed_from_playlist") {
    return "orange";
  }
  if (action === "instant_import") {
    return "cyan";
  }
  return "red";
}

function i18nKey(action: string): string {
  const map: Record<string, string> = {
    added_local: i18n.t("syncPage.action.addedLocal"),
    quarantined_local: i18n.t("syncPage.action.quarantinedLocal"),
    added_playlist: i18n.t("syncPage.action.addedPlaylist"),
    removed_from_playlist: i18n.t("syncPage.action.removedFromPlaylist"),
    added_cloud: i18n.t("syncPage.action.addedCloud"),
    instant_import: i18n.t("syncPage.action.instantImport"),
    failed: i18n.t("syncPage.action.failed"),
  };
  return map[action] ?? action;
}

/** 某次同步任务的变更明细（按 run 从数据库读取），含下载/上传/删除记录与恢复操作。 */
function RunChangesTable({ runId }: { runId: number }) {
  const { t } = useTranslation();
  const [rows, setRows] = useState<RunChangeEntry[] | null>(null);
  const [keyword, setKeyword] = useState("");
  const [actionFilter, setActionFilter] = useState("");
  const tri = useTriSort();
  // 最近一次云盘任务的比对记录只存内存（已在云盘/重复/未识别不落库）：
  // 该 run 的详情直接渲染内存明细，实时更新且信息完整；更早的任务查数据库。
  const currentRunId = useSyncExternalStore(cloudStore.subscribeRunId, cloudStore.getRunId);
  const liveRows = useSyncExternalStore(cloudStore.subscribeRows, cloudStore.getRows);
  const useLive = currentRunId === runId && liveRows.length > 0;

  const load = useCallback(async () => {
    try {
      setRows(await api.getRunChanges(runId));
    } catch (e) {
      antMessage.error(formatError(e));
      setRows([]);
    }
  }, [runId]);

  useEffect(() => {
    load();
  }, [load]);

  // 数据库模式下任务进行中每 2 秒刷新一次，详情随任务推进实时填充。
  const syncRunning = useSyncExternalStore(syncStore.subscribeRunning, syncStore.getRunning);
  const cloudRunning = useSyncExternalStore(cloudStore.subscribeRunning, cloudStore.getRunning);
  useEffect(() => {
    if (useLive || (!syncRunning && !cloudRunning)) return;
    const timer = setInterval(load, 2000);
    return () => clearInterval(timer);
  }, [load, useLive, syncRunning, cloudRunning]);

  if (useLive) {
    return <CloudTaskRowsTable rows={liveRows} />;
  }

  const restore = async (r: RunChangeEntry) => {
    if (!r.restoreId) return;
    try {
      if (r.restoreKind === "quarantine") await api.restoreQuarantine(r.restoreId);
      else await api.restoreDeletedItem(r.restoreId);
      antMessage.success(t("quarantine.restored"));
      load();
    } catch (e) {
      antMessage.error(t("quarantine.restoreFailed", { detail: formatError(e) }));
    }
  };

  const filtered = useMemo(() => {
    if (!rows) return [];
    const kw = keyword.trim().toLowerCase();
    return rows.filter((c) => {
      if (actionFilter && c.action !== actionFilter) return false;
      if (
        kw &&
        !`${c.trackName ?? ""} ${c.localPath ?? ""} ${c.quarantinedPath ?? ""}`
          .toLowerCase()
          .includes(kw)
      ) {
        return false;
      }
      return true;
    });
  }, [rows, keyword, actionFilter]);

  const sorted = useMemo(
    () =>
      applyTriSort(filtered, tri.sort, (c) => {
        switch (tri.sort?.key) {
          case "ts":
            return c.ts;
          case "action":
            return c.action;
          case "track":
            return c.trackName ?? "";
          case "direction":
            return c.direction;
          default:
            return "";
        }
      }),
    [filtered, tri.sort]
  );

  // 操作筛选按本次任务实际出现的操作动态生成。
  const actionOptions = useMemo(() => {
    const present: string[] = [];
    for (const r of rows ?? []) {
      if (!present.includes(r.action)) present.push(r.action);
    }
    return present.map((a) => ({ value: a, label: i18nKey(a) }));
  }, [rows]);

  const columns: ColumnsType<RunChangeEntry> = [
    {
      title: <SortHeader label={t("syncPage.colTime")} field="ts" tri={tri} />,
      dataIndex: "ts",
      width: 150,
      render: (v: string) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>{v}</Typography.Text>
      ),
    },
    {
      title: <SortHeader label={t("syncPage.colAction")} field="action" tri={tri} />,
      dataIndex: "action",
      width: 130,
      render: (action: string) => <Tag color={actionColor(action)}>{i18nKey(action)}</Tag>,
    },
    {
      title: <SortHeader label={t("syncPage.colTrack")} field="track" tri={tri} />,
      dataIndex: "trackName",
      ellipsis: true,
      render: (v: string | undefined, c) => (
        <div>
          <div>{v ?? c.trackId ?? "-"}</div>
          {c.action === "failed" && c.note && (
            <Typography.Text type="danger" style={{ fontSize: 12 }}>
              {renderMessage(c.note)}
            </Typography.Text>
          )}
        </div>
      ),
    },
    {
      title: t("syncPage.colPlaylist"),
      dataIndex: "playlistName",
      width: 130,
      ellipsis: true,
    },
    {
      title: <SortHeader label={t("syncPage.colDirection")} field="direction" tri={tri} />,
      dataIndex: "direction",
      width: 100,
      render: (d: string) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {i18n.t(`syncPage.direction.${d}`, { defaultValue: d })}
        </Typography.Text>
      ),
    },
    {
      title: t("syncPage.restore"),
      key: "restore",
      width: 100,
      render: (_, r) =>
        r.restoreId ? (
          <Popconfirm
            title={t("syncPage.restoreConfirmTitle")}
            okText={t("playlists.ok")}
            cancelText={t("playlists.cancel")}
            onConfirm={() => restore(r)}
          >
            <Button size="small" icon={<UndoOutlined />}>
              {t("syncPage.restore")}
            </Button>
          </Popconfirm>
        ) : null,
    },
  ];

  if (rows === null) {
    return <Typography.Text type="secondary">{t("settings.loading")}</Typography.Text>;
  }
  if (rows.length === 0) {
    return <Typography.Text type="secondary">{t("syncPage.runEmpty")}</Typography.Text>;
  }

  return (
    <>
      <Space style={{ marginBottom: 12 }} wrap>
        <Input.Search
          placeholder={t("syncPage.filterKeyword")}
          allowClear
          style={{ width: 220 }}
          onSearch={setKeyword}
          onChange={(e) => !e.target.value && setKeyword("")}
        />
        <Select
          style={{ width: 160 }}
          placeholder={t("syncPage.filterAction")}
          allowClear
          value={actionFilter || undefined}
          onChange={(v) => setActionFilter(v ?? "")}
          options={actionOptions}
        />
      </Space>
      <Table<RunChangeEntry>
        size="small"
        rowKey="id"
        dataSource={sorted}
        columns={columns}
        pagination={{
          pageSize: 20,
          showSizeChanger: true,
          pageSizeOptions: [20, 50, 100],
          showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}`,
        }}
      />
    </>
  );
}