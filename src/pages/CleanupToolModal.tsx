import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Checkbox,
  List,
  Modal,
  Progress,
  Space,
  Tag,
  Typography,
  message as antMessage,
} from "antd";
import { FolderOpenOutlined as FolderIcon } from "@ant-design/icons";
import { open } from "@tauri-apps/plugin-dialog";
import i18n from "../i18n";
import { api } from "../api";
import { formatError } from "../errors";
import type { CleanupScanReport, ToolControl, ToolProgress } from "../types";

const KIND = "cleanup_scan";

/** 字节数展示（KB/MB/GB）。 */
function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(2)} GB`;
  if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${bytes} B`;
}

/** 重复文件清理工具弹窗：选目录 → 后台扫描重复 → 确认后把建议项移入隔离区。 */
export default function CleanupToolModal({ open: isOpen, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = i18n;
  const [dirs, setDirs] = useState<string[]>([]);
  const [recursive, setRecursive] = useState(true);
  const [keepQuality, setKeepQuality] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [cleaning, setCleaning] = useState(false);
  const [scanProgress, setScanProgress] = useState<ToolProgress | null>(null);
  const [report, setReport] = useState<CleanupScanReport | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // 打开时若扫描仍在后台进行，接上进度。
  useEffect(() => {
    if (!isOpen) return;
    api
      .getToolControl(KIND)
      .then((control: ToolControl) => {
        setScanning(control.running);
        setScanProgress(control.progress);
        const result = control.result as CleanupScanReport | null | undefined;
        if (!control.running && result && "groups" in result) {
          setReport(result);
        }
      })
      .catch(() => {});
  }, [isOpen]);

  const stopPolling = useCallback(() => {
    if (pollRef.current) {
      clearInterval(pollRef.current);
      pollRef.current = null;
    }
  }, []);

  useEffect(() => stopPolling, [stopPolling]);

  useEffect(() => {
    if (!scanning) {
      stopPolling();
      return;
    }
    const poll = async () => {
      try {
        const control = await api.getToolControl(KIND);
        setScanProgress(control.progress);
        if (!control.running && control.result) {
          const result = control.result as CleanupScanReport;
          if ("groups" in result) setReport(result);
          setScanning(false);
          stopPolling();
        }
      } catch {
        // 忽略轮询失败
      }
    };
    poll();
    pollRef.current = setInterval(poll, 800);
    return stopPolling;
  }, [scanning, stopPolling]);

  const close = () => {
    if (scanning || cleaning) return;
    setReport(null);
    onClose();
  };

  const addDir = async () => {
    const picked = (await open({
      multiple: false,
      directory: true,
      title: t("settings.cleanupPickDir"),
    })) as string | string[] | null;
    if (!picked) return;
    const dir = (Array.isArray(picked) ? picked[0] : picked) ?? null;
    if (!dir) return;
    setDirs((prev) => (prev.includes(dir) ? prev : [...prev, dir]));
  };

  const removeDir = (d: string) => setDirs((prev) => prev.filter((x) => x !== d));

  const scan = async () => {
    if (dirs.length === 0) {
      antMessage.warning(t("errors.cleanupNoDirs"));
      return;
    }
    if (scanning) return;
    setReport(null);
    try {
      await api.startCleanupScan(dirs, recursive, keepQuality);
      setScanning(true);
    } catch (e) {
      antMessage.error(formatError(e));
    }
  };

  const cleanPaths =
    report?.groups.flatMap((g) => g.items.filter((i) => i.action === "clean").map((i) => i.path)) ??
    [];

  const execute = () => {
    if (cleanPaths.length === 0 || cleaning) return;
    Modal.confirm({
      title: t("settings.cleanupConfirmTitle"),
      content: t("settings.cleanupConfirmDesc", {
        count: cleanPaths.length,
        size: formatBytes(report?.cleanBytes ?? 0),
      }),
      okText: t("settings.cleanupExecute"),
      cancelText: t("settings.cancel"),
      okButtonProps: { danger: true },
      onOk: async () => {
        setCleaning(true);
        try {
          const count = await api.cleanupExecute(cleanPaths);
          antMessage.success(t("settings.cleanupDone", { count }));
          setReport(null);
        } catch (e) {
          antMessage.error(formatError(e));
        } finally {
          setCleaning(false);
        }
      },
    });
  };

  const duplicates = report?.groups.filter((g) => g.items.length > 1) ?? [];
  const chipStyle = { fontSize: 10, lineHeight: "16px", padding: "0 4px", marginInlineEnd: 0 } as const;

  return (
    <Modal
      title={t("settings.cleanupTitle")}
      open={isOpen}
      onCancel={close}
      width={760}
      footer={[
        <Button key="close" onClick={close} disabled={scanning || cleaning}>
          {t("tools.close")}
        </Button>,
        <Button
          key="scan"
          type="primary"
          loading={scanning}
          disabled={scanning || cleaning}
          onClick={scan}
        >
          {t("settings.cleanupScan")}
        </Button>,
        ...(cleanPaths.length > 0
          ? [
              <Button key="exec" type="primary" danger loading={cleaning} onClick={execute}>
                {t("settings.cleanupExecute")}
              </Button>,
            ]
          : []),
      ]}
    >
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        <Space>
          <Button icon={<FolderIcon />} onClick={addDir} disabled={scanning || cleaning}>
            {t("settings.cleanupPickDir")}
          </Button>
          {dirs.length > 0 && (
            <Typography.Text type="secondary">
              {t("settings.cleanupDirCount", { count: dirs.length })}
            </Typography.Text>
          )}
        </Space>
        {dirs.length > 0 && (
          <List
            size="small"
            bordered
            dataSource={dirs}
            style={{ maxHeight: 120, overflow: "auto" }}
            renderItem={(d) => (
              <List.Item
                actions={[
                  <Button
                    key="remove"
                    type="text"
                    size="small"
                    danger
                    disabled={scanning || cleaning}
                    onClick={() => removeDir(d)}
                  >
                    {t("tools.remove")}
                  </Button>,
                ]}
              >
                <Typography.Text ellipsis style={{ maxWidth: 620, fontSize: 12 }}>
                  {d}
                </Typography.Text>
              </List.Item>
            )}
          />
        )}
        <Space size="large">
          <Checkbox
            checked={recursive}
            onChange={(e) => setRecursive(e.target.checked)}
            disabled={scanning || cleaning}
          >
            {t("settings.cleanupRecursive")}
          </Checkbox>
          <Checkbox
            checked={keepQuality}
            onChange={(e) => setKeepQuality(e.target.checked)}
            disabled={scanning || cleaning}
          >
            {t("settings.cleanupKeepQuality")}
          </Checkbox>
        </Space>
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          {t("settings.cleanupRuleHint")}
        </Typography.Text>
        {scanning && (
          <>
            <Progress
              percent={
                scanProgress && scanProgress.total > 0
                  ? Math.round((scanProgress.ok / scanProgress.total) * 100)
                  : 0
              }
              status="active"
            />
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {t("settings.cleanupScanning")}
            </Typography.Text>
          </>
        )}
        {report && (
          <Alert
            type={report.cleanCount > 0 ? "warning" : "success"}
            showIcon
            message={
              report.cleanCount > 0
                ? t("settings.cleanupSummaryFound", {
                    scanned: report.scannedFiles,
                    groups: report.duplicateGroups,
                    count: report.cleanCount,
                    size: formatBytes(report.cleanBytes),
                  })
                : t("settings.cleanupSummaryNone", { scanned: report.scannedFiles })
            }
            description={
              report.unresolved > 0
                ? t("settings.cleanupUnresolvedTip", { count: report.unresolved })
                : undefined
            }
          />
        )}
        {duplicates.length > 0 && (
          <div style={{ maxHeight: 340, overflow: "auto" }}>
            {duplicates.map((g, gi) => (
              <div key={gi} style={{ marginBottom: 14 }}>
                <Typography.Text strong style={{ fontSize: 13 }}>
                  {g.song}
                </Typography.Text>
                <List
                  size="small"
                  dataSource={g.items}
                  renderItem={(item) => (
                    <List.Item
                      style={{ padding: "4px 0" }}
                      extra={
                        item.action === "clean" ? (
                          <Tag color="orange">{t("settings.cleanupClean")}</Tag>
                        ) : (
                          <Tag color="green">{t("settings.cleanupKeep")}</Tag>
                        )
                      }
                    >
                      <Space size={8} wrap>
                        <Tag>{item.quality}</Tag>
                        <Typography.Text ellipsis style={{ maxWidth: 360, fontSize: 12 }} type="secondary">
                          {item.path}
                        </Typography.Text>
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          {formatBytes(item.size)} ·{" "}
                          {item.bitrate > 0 ? `${Math.round(item.bitrate / 1000)}kbps · ` : ""}
                          {item.modifiedAt} · {t("settings.cleanupScore", { score: item.score })}
                        </Typography.Text>
                        <Space size={2}>
                          {item.hasComment && <Tag style={chipStyle}>{t("settings.cleanupTagComment")}</Tag>}
                          {item.hasCover && <Tag style={chipStyle}>{t("settings.cleanupTagCover")}</Tag>}
                          {item.hasLyrics && <Tag style={chipStyle}>{t("settings.cleanupTagLyrics")}</Tag>}
                          {item.hasSidecar && <Tag style={chipStyle}>{t("settings.cleanupTagSidecar")}</Tag>}
                        </Space>
                      </Space>
                    </List.Item>
                  )}
                />
              </div>
            ))}
          </div>
        )}
      </Space>
    </Modal>
  );
}
