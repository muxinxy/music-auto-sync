import { useCallback, useEffect, useState } from "react";
import { Card, List, Progress, Tag, Typography, message as antMessage } from "antd";
import { listen } from "@tauri-apps/api/event";
import { api } from "../api";
import type { SyncProgress, SyncReport } from "../types";

interface LogEntry {
  id: number;
  ts: string;
  playlistName: string;
  status: string;
  message: string;
}

function statusTag(s: string) {
  if (s === "ok") return <Tag color="success">成功</Tag>;
  if (s === "error") return <Tag color="error">失败</Tag>;
  return <Tag color="processing">{s}</Tag>;
}

export default function SyncPage() {
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const [reports, setReports] = useState<SyncReport[]>([]);
  const [logs, setLogs] = useState<LogEntry[]>([]);

  const loadLogs = useCallback(async () => {
    try {
      setLogs(await api.getSyncLogs(100));
    } catch (e) {
      antMessage.error(`加载日志失败：${e}`);
    }
  }, []);

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

  return (
    <div style={{ padding: 24 }}>
      <Card title="当前任务" style={{ marginBottom: 16 }}>
        {progress ? (
          <>
            <Typography.Paragraph>
              <Tag color="processing">{progress.phase}</Tag>
              {progress.playlistName} —— {progress.message}
            </Typography.Paragraph>
            <Progress
              percent={progress.total ? Math.round((progress.current / progress.total) * 100) : 0}
              status="active"
            />
          </>
        ) : (
          <Typography.Text type="secondary">暂无正在进行的同步任务</Typography.Text>
        )}
      </Card>

      {reports.length > 0 && (
        <Card title="最近结果" style={{ marginBottom: 16 }} size="small">
          <List
            size="small"
            dataSource={reports}
            renderItem={(r) => (
              <List.Item>
                <Typography.Text>
                  {r.playlistName}：新增 {r.added} · 转换 {r.ncmConverted} · 隔离{" "}
                  {r.quarantined} · 失败 {r.failed}
                  {r.errors.length > 0 && (
                    <Typography.Text type="danger">（{r.errors[0]}）</Typography.Text>
                  )}
                </Typography.Text>
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {r.finishedAt}
                </Typography.Text>
              </List.Item>
            )}
          />
        </Card>
      )}

      <Card title="同步日志" styles={{ body: { padding: 0 } }}>
        <List
          size="small"
          dataSource={logs}
          locale={{ emptyText: "暂无日志" }}
          renderItem={(l) => (
            <List.Item style={{ paddingLeft: 24, paddingRight: 24 }}>
              <List.Item.Meta
                title={
                  <Typography.Text style={{ fontSize: 13 }}>
                    {statusTag(l.status)} {l.playlistName || "-"}
                  </Typography.Text>
                }
                description={l.message}
              />
              <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                {l.ts}
              </Typography.Text>
            </List.Item>
          )}
        />
      </Card>
    </div>
  );
}
