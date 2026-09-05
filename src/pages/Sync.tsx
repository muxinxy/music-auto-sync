import { useCallback, useEffect, useState } from "react";
import {
  Button,
  Card,
  Collapse,
  Input,
  List,
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
import { RestOutlined, UndoOutlined } from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import i18n from "../i18n";
import { api } from "../api";
import { formatError, translateUi } from "../errors";
import type {
  DeletedLogEntry,
  SyncChangeEntry,
  SyncErrorDetail,
  SyncProgress,
  SyncReport,
  UiMessage,
} from "../types";

interface LogEntry {
  id: number;
  ts: string;
  playlistName: string;
  status: string;
  message: string;
}

function renderMessage(raw: string): string {
  if (raw.startsWith("{")) {
    try {
      return translateUi(JSON.parse(raw) as UiMessage);
    } catch {
      return raw;
    }
  }
  return raw;
}

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
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const [reports, setReports] = useState<SyncReport[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [changes, setChanges] = useState<SyncChangeEntry[]>([]);
  const [deleted, setDeleted] = useState<DeletedLogEntry[]>([]);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [changeFilter, setChangeFilter] = useState("");
  const [changeAction, setChangeAction] = useState<string>("");
  const [logFilter, setLogFilter] = useState("");
  const [deletedFilter, setDeletedFilter] = useState("");
  const [deletedKind, setDeletedKind] = useState<string>("");

  const loadLogs = useCallback(async () => {
    try {
      setLogs(await api.getSyncLogs(1000));
    } catch (e) {
      antMessage.error(t("syncPage.loadLogsFailed", { detail: String(e) }));
    }
  }, [t]);

  const loadChanges = useCallback(async () => {
    try {
      const [c, d] = await Promise.all([api.getSyncChanges(2000), api.getDeletedLog(2000)]);
      setChanges(c);
      setDeleted(d);
    } catch {
      // 静默
    }
  }, []);

  useEffect(() => {
    loadLogs();
    loadChanges();
    const un1 = listen<SyncProgress>("sync://progress", (e) => setProgress(e.payload));
    const un2 = listen<SyncReport>("sync://report", (e) => {
      setReports((r) => [e.payload, ...r].slice(0, 20));
      loadLogs();
      loadChanges();
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
    };
  }, [loadLogs, loadChanges]);

  const restoreDeleted = async (item: DeletedLogEntry) => {
    try {
      const label = await api.restoreDeletedItem(item.id);
      antMessage.success(t("syncPage.restored", { name: label }));
      loadChanges();
    } catch (e) {
      antMessage.error(formatError(e));
    }
  };

  const clearHistory = async (kind: "logs" | "changes" | "deleted") => {
    try {
      const count = await api.clearSyncHistory(kind);
      antMessage.success(t("syncPage.cleared", { count }));
      loadLogs();
      loadChanges();
    } catch (e) {
      antMessage.error(formatError(e));
    }
  };

  const phaseLabel = progress
    ? t(`phases.${progress.phase}`, { defaultValue: progress.phase })
    : "";
  const progressMessage =
    progress?.message.code === "track" && progress.message.params?.[0]
      ? progress.message.params[0]
      : progress?.message
        ? translateUi(progress.message)
        : "";

  return (
    <div style={{ padding: 24 }}>
      <Card title={t("syncPage.currentTask")} style={{ marginBottom: 16 }}>
        {progress ? (
          <>
            <Typography.Paragraph>
              <Tag color="processing">{phaseLabel}</Tag>
              {progress.playlistName} —— {progressMessage}
            </Typography.Paragraph>
            <Progress
              percent={progress.total ? Math.round((progress.current / progress.total) * 100) : 0}
              status="active"
            />
          </>
        ) : (
          <Typography.Text type="secondary">{t("syncPage.noTask")}</Typography.Text>
        )}
      </Card>

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
                    hasDetails ? (
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
                          name: r.playlistName,
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
                                {d.trackName}：{translateUi(d.message)}
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
        styles={{ body: { padding: 0 } }}
        extra={
          logs.length > 0 ? (
            <Popconfirm
              title={t("syncPage.clearConfirm")}
              okText={t("syncPage.clearLogs")}
              cancelText={t("playlists.cancel")}
              onConfirm={() => clearHistory("logs")}
            >
              <Button size="small" type="text">
                {t("syncPage.clearLogs")}
              </Button>
            </Popconfirm>
          ) : undefined
        }
      >
        <div style={{ padding: 12 }}>
          <Input.Search
            placeholder={t("syncPage.filterLog")}
            allowClear
            style={{ width: 260 }}
            onSearch={setLogFilter}
            onChange={(e) => !e.target.value && setLogFilter("")}
          />
        </div>
        <List
          size="small"
          dataSource={logs.filter((l) => {
            if (!logFilter) return true;
            const kw = logFilter.toLowerCase();
            const hay = `${l.playlistName} ${renderMessage(l.message)}`.toLowerCase();
            return hay.includes(kw);
          })}
          pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: [20, 50, 100], showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}` }}
          locale={{ emptyText: t("syncPage.noLogs") }}
          renderItem={(l) => {
            const tag =
              l.status === "ok" ? (
                <Tag color="success">{t("syncPage.statusSuccess")}</Tag>
              ) : l.status === "error" ? (
                <Tag color="error">{t("syncPage.statusFailed")}</Tag>
              ) : (
                <Tag color="processing">{t("syncPage.statusRunning")}</Tag>
              );
            return (
              <List.Item style={{ paddingLeft: 24, paddingRight: 24 }}>
                <List.Item.Meta
                  title={
                    <Typography.Text style={{ fontSize: 13 }}>
                      {tag} {l.playlistName || "-"}
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

      <Card
        title={t("syncPage.changesTitle")}
        style={{ marginBottom: 16 }}
        size="small"
        extra={
          changes.length > 0 ? (
            <Popconfirm
              title={t("syncPage.clearConfirm")}
              okText={t("syncPage.clearChanges")}
              cancelText={t("playlists.cancel")}
              onConfirm={() => clearHistory("changes")}
            >
              <Button size="small" type="text">
                {t("syncPage.clearChanges")}
              </Button>
            </Popconfirm>
          ) : undefined
        }
      >
        <Space style={{ marginBottom: 12 }} wrap>
          <Input.Search
            placeholder={t("syncPage.filterKeyword")}
            allowClear
            style={{ width: 220 }}
            onSearch={setChangeFilter}
            onChange={(e) => !e.target.value && setChangeFilter("")}
          />
          <Select
            style={{ width: 160 }}
            placeholder={t("syncPage.filterAction")}
            allowClear
            value={changeAction || undefined}
            onChange={(v) => setChangeAction(v ?? "")}
            options={[
              { value: "added_local", label: t("syncPage.action.addedLocal") },
              { value: "quarantined_local", label: t("syncPage.action.quarantinedLocal") },
              { value: "added_playlist", label: t("syncPage.action.addedPlaylist") },
              { value: "removed_from_playlist", label: t("syncPage.action.removedFromPlaylist") },
            ]}
          />
        </Space>
        <Table<SyncChangeEntry>
          size="small"
          rowKey="id"
          dataSource={changes.filter((c) => {
            if (changeAction && c.action !== changeAction) return false;
            if (changeFilter) {
              const kw = changeFilter.toLowerCase();
              const hay = `${c.playlistName} ${c.trackName ?? ""} ${c.localPath ?? ""}`.toLowerCase();
              if (!hay.includes(kw)) return false;
            }
            return true;
          })}
          columns={buildChangeColumns()}
          pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: [20, 50, 100] }}
          locale={{ emptyText: t("syncPage.noChanges") }}
        />
      </Card>

      <Card
        title={t("syncPage.deletedTitle")}
        size="small"
        extra={
          deleted.length > 0 ? (
            <Popconfirm
              title={t("syncPage.clearConfirm")}
              okText={t("syncPage.clearDeleted")}
              cancelText={t("playlists.cancel")}
              onConfirm={() => clearHistory("deleted")}
            >
              <Button size="small" type="text">
                {t("syncPage.clearDeleted")}
              </Button>
            </Popconfirm>
          ) : undefined
        }
      >
        <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
          {t("syncPage.deletedExplain")}
        </Typography.Paragraph>
        <Space style={{ marginBottom: 12 }} wrap>
          <Input.Search
            placeholder={t("syncPage.filterDeleted")}
            allowClear
            style={{ width: 240 }}
            onSearch={setDeletedFilter}
            onChange={(e) => !e.target.value && setDeletedFilter("")}
          />
          <Select
            style={{ width: 150 }}
            placeholder={t("syncPage.filterKind")}
            allowClear
            value={deletedKind || undefined}
            onChange={(v) => setDeletedKind(v ?? "")}
            options={[
              { value: "local_file", label: t("syncPage.deletedLocal") },
              { value: "playlist_track", label: t("syncPage.deletedPlaylist") },
            ]}
          />
        </Space>
        <Table<DeletedLogEntry>
          size="small"
          rowKey="id"
          dataSource={deleted.filter((d) => {
            if (deletedKind && d.kind !== deletedKind) return false;
            if (deletedFilter) {
              const kw = deletedFilter.toLowerCase();
              const hay = `${d.playlistName} ${d.trackName ?? ""} ${d.localPath ?? ""}`.toLowerCase();
              if (!hay.includes(kw)) return false;
            }
            return true;
          })}
          columns={deletedColumns(restoreDeleted, t)}
          pagination={{ pageSize: 20, showSizeChanger: true, pageSizeOptions: [20, 50, 100], showTotal: (total, range) => `${range[0]}-${range[1]} / ${total}` }}
          locale={{ emptyText: t("syncPage.noDeleted") }}
        />
      </Card>
    </div>
  );
}

function buildChangeColumns(): ColumnsType<SyncChangeEntry> {
  return [
    {
      title: i18n.t("syncPage.colTime"),
      dataIndex: "ts",
      width: 150,
      render: (v: string) => <Typography.Text type="secondary" style={{ fontSize: 12 }}>{v}</Typography.Text>,
    },
    {
      title: i18n.t("syncPage.colAction"),
      dataIndex: "action",
      width: 130,
      render: (action: string) => {
        const color =
          action === "added_local" || action === "added_playlist"
            ? "green"
            : action === "quarantined_local" || action === "removed_from_playlist"
              ? "orange"
              : "red";
        return <Tag color={color}>{i18nKey(action)}</Tag>;
      },
    },
    {
      title: i18n.t("syncPage.colTrack"),
      dataIndex: "trackName",
      ellipsis: true,
      render: (v: string | undefined, c) => v ?? c.note ?? c.trackId ?? "-",
    },
    {
      title: i18n.t("syncPage.colPlaylist"),
      dataIndex: "playlistName",
      width: 160,
      ellipsis: true,
    },
    {
      title: i18n.t("syncPage.colDirection"),
      dataIndex: "direction",
      width: 110,
      render: (d: string) => (
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {i18n.t(`syncPage.direction.${d}`, { defaultValue: d })}
        </Typography.Text>
      ),
    },
  ];
}

function i18nKey(action: string): string {
  const map: Record<string, string> = {
    added_local: i18n.t("syncPage.action.addedLocal"),
    quarantined_local: i18n.t("syncPage.action.quarantinedLocal"),
    added_playlist: i18n.t("syncPage.action.addedPlaylist"),
    removed_from_playlist: i18n.t("syncPage.action.removedFromPlaylist"),
    failed: i18n.t("syncPage.action.failed"),
  };
  return map[action] ?? action;
}

function deletedColumns(
  restoreDeleted: (d: DeletedLogEntry) => void,
  t: (k: string, opts?: Record<string, unknown>) => string
): ColumnsType<DeletedLogEntry> {
  return [
    {
      title: t("syncPage.colTime"),
      dataIndex: "ts",
      width: 150,
      render: (v: string) => <Typography.Text type="secondary" style={{ fontSize: 12 }}>{v}</Typography.Text>,
    },
    {
      title: t("syncPage.colType"),
      dataIndex: "kind",
      width: 110,
      render: (k: string) => (
        <Tag color={k === "local_file" ? "geekblue" : "volcano"}>
          {k === "local_file" ? t("syncPage.deletedLocal") : t("syncPage.deletedPlaylist")}
        </Tag>
      ),
    },
    {
      title: t("syncPage.colName"),
      key: "name",
      ellipsis: true,
      render: (_, d) => d.trackName ?? d.localPath ?? "-",
    },
    {
      title: t("syncPage.colPlaylist"),
      dataIndex: "playlistName",
      width: 150,
      ellipsis: true,
    },
    {
      title: "",
      key: "action",
      width: 110,
      render: (_, d) =>
        !d.restoredAt ? (
          <Popconfirm
            title={t("syncPage.restoreConfirmTitle")}
            okText={t("playlists.ok")}
            cancelText={t("playlists.cancel")}
            onConfirm={() => restoreDeleted(d)}
          >
            <Button size="small" icon={<UndoOutlined />}>
              {t("syncPage.restore")}
            </Button>
          </Popconfirm>
        ) : (
          <Tag color="success">{t("syncPage.restoredTag")}</Tag>
        ),
    },
  ];
}