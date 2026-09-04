import { useCallback, useEffect, useState } from "react";
import { Button, Card, List, Progress, Tag, Typography, message as antMessage } from "antd";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { translateUi } from "../errors";
import type { SyncErrorDetail, SyncProgress, SyncReport, UiMessage } from "../types";

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

export default function SyncPage() {
  const { t } = useTranslation();
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const [reports, setReports] = useState<SyncReport[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [expanded, setExpanded] = useState<string | null>(null);

  const loadLogs = useCallback(async () => {
    try {
      setLogs(await api.getSyncLogs(100));
    } catch (e) {
      antMessage.error(t("syncPage.loadLogsFailed", { detail: String(e) }));
    }
  }, [t]);

  useEffect(() => {
    loadLogs();
    const un1 = listen<SyncProgress>("sync://progress", (e) => setProgress(e.payload));
    const un2 = listen<SyncReport>("sync://report", (e) => {
      setReports((r) => [e.payload, ...r].slice(0, 20));
      loadLogs();
    });
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
    };
  }, [loadLogs]);

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

      <Card title={t("syncPage.syncLogs")} styles={{ body: { padding: 0 } }}>
        <List
          size="small"
          dataSource={logs}
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
    </div>
  );
}