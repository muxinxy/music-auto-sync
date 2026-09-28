import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import type { ReactNode } from "react";
import {
  Button,
  Card,
  DatePicker,
  Input,
  List,
  Modal,
  Pagination,
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
import { toolStore, type ToolState } from "../toolStore";
import type { CloudTaskRow, RunChangeEntry, SyncErrorDetail, SyncProgress, SyncReport, UiMessage } from "../types";
import { listPagination, tablePagination } from "../listConfig";
import SearchWithHistory from "../SearchWithHistory";
import { loadFilters, saveFilters } from "../filterMemory";

interface LogEntry {
  id: number;
  ts: string;
  playlistName: string;
  status: string;
  message: string;
}

/** 移除翻译后残留的未填充占位符（如 {3}、{{3}}）。 */
function stripLeftoverPlaceholders(text: string): string {
  return text.replace(/\{\{?\d+\}?\}/g, "").trim();
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
    result = stripLeftoverPlaceholders(result);
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
  const savedLogFilters = loadFilters("filters.syncLogs", { logFilter: "", taskType: "" });
  const [logFilter, setLogFilter] = useState(savedLogFilters.logFilter);
  const [taskType, setTaskType] = useState(savedLogFilters.taskType);
  const [logPage, setLogPage] = useState(1);
  const [logPageSize, setLogPageSize] = useState(20);
  const taskTypeOptions = [
    { value: "sync", label: t("syncPage.taskType.sync") },
    { value: "cloud", label: taskDisplayName("cloud") },
    { value: "cloud_download", label: taskDisplayName("cloud_download") },
    { value: "ncm_convert", label: taskDisplayName("ncm_convert") },
    { value: "cleanup_scan", label: taskDisplayName("cleanup_scan") },
    { value: "cleanup", label: taskDisplayName("cleanup") },
    { value: "repair", label: taskDisplayName("repair") },
  ];
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
    // 工具箱任务（NCM 转换/重复清理/属性修复）结束 → 刷新日志。
    const un6 = listen<{ kind: string; running: boolean }>("tool://state", (e) => {
      if (!e.payload.running) setTimeout(loadLogs, 300);
    });
    // 全局轮询检测到工具任务结束（兜底）→ 刷新日志。
    const onToolFinished = () => setTimeout(loadLogs, 300);
    window.addEventListener("tool-task-finished", onToolFinished);
    return () => {
      un2.then((f) => f());
      un3.then((f) => f());
      un4.then((f) => f());
      un5.then((f) => f());
      un6.then((f) => f());
      window.removeEventListener("tool-task-finished", onToolFinished);
    };
  }, [loadLogs]);

  useEffect(() => {
    saveFilters("filters.syncLogs", { logFilter, taskType });
  }, [logFilter, taskType]);

  const filteredLogs = logs.filter((l) => {
    if (logRange && logRange[0] && logRange[1]) {
      const ts = dayjs(l.ts);
      if (ts.isBefore(logRange[0].startOf("second")) || ts.isAfter(logRange[1].endOf("second"))) {
        return false;
      }
    }
    if (taskType) {
      // 同步歌单的日志用真实歌单名，其余为任务哨兵名。
      const sentinels = ["cloud", "cloud_download", "ncm_convert", "cleanup_scan", "cleanup", "repair"];
      if (taskType === "sync") {
        if (sentinels.includes(l.playlistName)) return false;
      } else if (l.playlistName !== taskType) {
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
  });
  const pagedLogs = filteredLogs.slice((logPage - 1) * logPageSize, logPage * logPageSize);

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
    <div style={{ padding: 24, height: "100%", display: "flex", flexDirection: "column", minHeight: 0 }}>
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
        <Card title={t("syncPage.recentResults")} style={{ marginBottom: 16, flexShrink: 0 }} size="small">
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
        style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column" }}
        styles={{
          body: {
            padding: 0,
            display: "flex",
            flexDirection: "column",
            flex: 1,
            minHeight: 0,
            overflow: "hidden",
          },
        }}
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
        <div style={{ padding: 12, flexShrink: 0 }}>
          <Space wrap>
            <SearchWithHistory
              storageKey="search.syncLogs"
              value={logFilter}
              onChange={setLogFilter}
              onSearch={(word) => {
                setLogFilter(word);
                setLogPage(1);
              }}
              placeholder={t("syncPage.filterLog")}
              style={{ width: 240 }}
            />
            <Select
              allowClear
              placeholder={t("syncPage.filterTaskType")}
              style={{ minWidth: 180 }}
              value={taskType || undefined}
              onChange={(v) => {
                setTaskType(v ?? "");
                setLogPage(1);
              }}
              options={taskTypeOptions}
            />
            <Button
              size="small"
              onClick={() => {
                setLogFilter("");
                setTaskType("");
                setLogRange(null);
                setLogPage(1);
              }}
            >
              {t("filters.clear")}
            </Button>
            <DatePicker.RangePicker
              showTime={{ format: "HH:mm" }}
              format="YYYY-MM-DD HH:mm"
              value={logRange}
              onChange={(value) => {
                setLogRange(value as [Dayjs | null, Dayjs | null] | null);
                setLogPage(1);
              }}
              allowClear
              size="small"
            />
          </Space>
        </div>
        <div style={{ flex: 1, overflowY: "auto", paddingLeft: 12, paddingRight: 12 }}>
        <List
          size="small"
          dataSource={pagedLogs}
          pagination={false}
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
                  description={
                    <Typography.Text
                      ellipsis={{ tooltip: renderMessage(l.message) }}
                      style={{ fontSize: 12, maxWidth: 720 }}
                    >
                      {renderMessage(l.message)}
                    </Typography.Text>
                  }
                />
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {l.ts}
                </Typography.Text>
              </List.Item>
            );
          }}
        />
        </div>
        <div
          style={{
            flexShrink: 0,
            padding: "8px 16px",
            borderTop: "1px solid rgba(128,128,128,0.2)",
            display: "flex",
            justifyContent: "flex-end",
          }}
        >
          <Pagination
            size="small"
            current={logPage}
            pageSize={logPageSize}
            total={filteredLogs.length}
            showSizeChanger
            pageSizeOptions={[20, 50, 100]}
            showTotal={(total, range) => `${range[0]}-${range[1]} / ${total}`}
            onChange={(page, size) => {
              setLogPage(size !== logPageSize ? 1 : page);
              setLogPageSize(size);
            }}
          />
        </div>
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
        pagination={tablePagination}
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
  const toolSnapshot = useSyncExternalStore(toolStore.subscribe, toolStore.getSnapshot);
  // 运行中，或刚结束 20 秒内（短任务一闪而过，保留展示便于确认结果）。
  const visibleTools = (Object.entries(toolSnapshot) as [string, ToolState][]).filter(([, s]) => {
    if (!s.progress) return false;
    if (s.running && !s.progress.done) return true;
    return s.progress.done && s.finishedAt !== undefined && Date.now() - s.finishedAt < 20000;
  });


  return (
    <Card title={t("syncPage.currentTask")} size="small" style={{ marginBottom: 16, flexShrink: 0 }}>
      {cloudActive || playlistActive || visibleTools.length > 0 ? (
        <Space direction="vertical" style={{ width: "100%" }} size={16}>
          {visibleTools.map(([kind, s]) => {
            const p = s.progress!;
            const paused = p.paused;
            const hasDone = p.done;
            const hasCanceled = p.canceled;
            return (
              <div key={kind}>
                <Typography.Paragraph style={{ marginBottom: 4 }}>
                  <Tag color={hasDone ? "success" : paused ? "warning" : "processing"}>
                    {hasDone
                      ? hasCanceled
                        ? t("app.cancel")
                        : t("tools.taskFinished")
                      : paused
                        ? t("tools.taskPaused")
                        : t("tools.taskRunning")}
                  </Tag>
                  {taskDisplayName(kind)} —— {hasDone ? t("tools.taskFinishedHint") : p.currentFile || t("tools.taskPreparing")}
                </Typography.Paragraph>
                <Progress
                  percent={p.total ? Math.round((p.current / p.total) * 100) : 0}
                  status={paused ? "normal" : "active"}
                />
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {t("tools.repairProgress", {
                    current: p.current,
                    total: p.total,
                    repaired: p.ok,
                    skipped: p.skipped,
                    failed: p.failed,
                  })}
                </Typography.Text>
                {!hasDone && (
                <Space style={{ marginTop: 6 }} wrap>
                  <Button
                    size="small"
                    onClick={async () => {
                      try {
                        if (paused) {
                          await api.resumeTool(kind);
                        } else {
                          await api.pauseTool(kind);
                        }
                      } catch (e) {
                        antMessage.error(formatError(e));
                      }
                    }}
                  >
                    {paused ? t("app.resume") : t("app.pause")}
                  </Button>
                  <Popconfirm
                    title={t("app.cancelConfirm")}
                    okText={t("app.cancel")}
                    cancelText={t("playlists.cancel")}
                    onConfirm={async () => {
                      try {
                        await api.cancelTool(kind);
                      } catch (e) {
                        antMessage.error(formatError(e));
                      }
                    }}
                  >
                    <Button size="small" danger>
                      {t("app.cancelTask")}
                    </Button>
                  </Popconfirm>
                </Space>
                )}
              </div>
            );
          })}
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
  if (action === "repaired" || action === "converted") {
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
    repaired: i18n.t("syncPage.action.repaired"),
    converted: i18n.t("syncPage.action.converted"),
    skipped: i18n.t("syncPage.action.skipped"),
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
  // 工具箱任务（属性修复/NCM/清理扫描）进行中同样实时刷新。
  const toolSnapshotForRun = useSyncExternalStore(toolStore.subscribe, toolStore.getSnapshot);
  const anyToolRunning = Object.values(toolSnapshotForRun).some((toolRow) => toolRow.running);
  useEffect(() => {
    if (useLive || (!syncRunning && !cloudRunning && !anyToolRunning)) return;
    const timer = setInterval(load, 2000);
    return () => clearInterval(timer);
  }, [load, useLive, syncRunning, cloudRunning, anyToolRunning]);

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
          <div>
            <Typography.Text
              ellipsis
              style={{ maxWidth: 220 }}
              title={v ?? String(c.trackId ?? "-")}
            >
              {v ?? c.trackId ?? "-"}
            </Typography.Text>
          </div>
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
      render: (name: string, c: RunChangeEntry) => {
        // 歌单同步：显示真实歌单名；工具任务：显示该文件所在目录名（任务名见弹窗标题）。
        const sentinels = ["cloud", "cloud_download", "ncm_convert", "cleanup_scan", "cleanup", "repair"];
        if (sentinels.includes(name)) {
          const path = c.localPath ?? c.quarantinedPath ?? "";
          const dir = path.replace(/[\\/][^\\/]*$/, "");
          const dirName = dir.replace(/.*[\\/]/, "");
          return dirName || "-";
        }
        return taskDisplayName(name) || "-";
      },
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
        scroll={{ y: "calc(100vh - 360px)" }}
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