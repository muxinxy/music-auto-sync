import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Checkbox,
  List,
  Modal,
  Progress,
  Space,
  Typography,
  message as antMessage,
} from "antd";
import { FileAddOutlined, FolderOpenOutlined } from "@ant-design/icons";
import { open } from "@tauri-apps/plugin-dialog";
import i18n from "../i18n";
import { api } from "../api";
import { formatError, translateItemError } from "../errors";
import type { ToolControl, RepairItemResult } from "../types";

const KIND = "ncm_convert";

/** 独立 NCM 转换工具弹窗：选文件（可多选）或目录 → 列出 .ncm → 后台转换。 */
export default function NcmToolModal({ open: isOpen, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = i18n;
  const [files, setFiles] = useState<string[]>([]);
  const [keepSource, setKeepSource] = useState(true);
  const [overwrite, setOverwrite] = useState(false);
  const [running, setRunning] = useState(false);
  const [control, setControl] = useState<ToolControl | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const resultsShownRef = useRef(false);

  useEffect(() => {
    if (!isOpen) return;
    api
      .getToolControl(KIND)
      .then((control) => {
        setControl(control);
        setRunning(control.running);
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
    if (!running) {
      stopPolling();
      return;
    }
    const poll = async () => {
      try {
        const control = await api.getToolControl(KIND);
        setControl(control);
        if (!control.running) {
          setRunning(false);
          stopPolling();
        }
      } catch {
        // 轮询失败忽略
      }
    };
    poll();
    pollRef.current = setInterval(poll, 800);
    return stopPolling;
  }, [running, stopPolling]);

  const close = () => {
    if (running) return;
    setFiles([]);
    setControl(null);
    onClose();
  };

  const addByFiles = async () => {
    const picked = (await open({
      multiple: true,
      filters: [{ name: "NCM", extensions: ["ncm"] }],
      title: t("tools.ncmPickFiles"),
    })) as string[] | string | null;
    if (!picked) return;
    const list = Array.isArray(picked) ? picked : [picked];
    setFiles((prev) => {
      const next = [...prev];
      for (const f of list) if (!next.includes(f)) next.push(f);
      return next;
    });
  };

  const addByDir = async () => {
    const picked = (await open({
      multiple: false,
      directory: true,
      title: t("tools.ncmPickDir"),
    })) as string | string[] | null;
    if (!picked) return;
    const dir = (Array.isArray(picked) ? picked[0] : picked) ?? null;
    if (!dir) return;
    setFiles((prev) => (prev.includes(dir) ? prev : [...prev, dir]));
  };

  const start = async () => {
    if (files.length === 0) {
      antMessage.warning(t("errors.ncmNoFiles"));
      return;
    }
    if (running) return;
    try {
      await api.startNcmConvert(files, keepSource, overwrite);
      setRunning(true);
      setFiles([]);
      resultsShownRef.current = true;
    } catch (e) {
      antMessage.error(formatError(e));
    }
  };

  const pauseResume = async () => {
    try {
      if (control?.progress.paused) {
        await api.resumeTool(KIND);
      } else {
        await api.pauseTool(KIND);
      }
      // 本地立即翻转暂停状态，不等下一轮轮询。
      setControl((prev) =>
        prev
          ? { ...prev, progress: { ...prev.progress, paused: !prev.progress.paused } }
          : prev
      );
    } catch (e) {
      antMessage.error(formatError(e));
    }
  };

  const cancel = async () => {
    try {
      await api.cancelTool(KIND);
    } catch (e) {
      antMessage.error(formatError(e));
    }
  };

  const progress = control?.progress;
  const items = (control?.items ?? []) as RepairItemResult[];
  const failureItems = items.filter((i) => i.status === "failed");
  const shownFailures = failureItems.slice(0, 6);
  const hiddenFailures = failureItems.length - shownFailures.length;

  return (
    <Modal
      title={t("tools.ncmTitle")}
      open={isOpen}
      onCancel={close}
      footer={
        running
          ? [
              <Button
                key="pause"
                onClick={pauseResume}
              >
                {control?.progress.paused ? t("app.resume") : t("app.pause")}
              </Button>,
              <Button key="cancel-task" danger onClick={cancel}>
                {t("tools.repairCancel")}
              </Button>,
              <Button key="close" type="primary" onClick={() => onClose()}>
                {t("tools.close")}
              </Button>,
            ]
          : [
              <Button key="close" onClick={close}>
                {t("tools.close")}
              </Button>,
              <Button
                key="start"
                type="primary"
                loading={running}
                onClick={start}
              >
                {t("tools.ncmStart")}
              </Button>,
            ]
      }
      width={640}
    >
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        {running ? (
          <>
            <Progress
              percent={progress && progress.total > 0 ? Math.round((progress.current / progress.total) * 100) : 0}
              status={
                progress?.canceled
                  ? "exception"
                  : progress?.done
                    ? "success"
                    : progress?.paused
                      ? "normal"
                      : "active"
              }
            />
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {progress?.paused ? <Typography.Text type="warning">{t("tools.taskPaused")} · </Typography.Text> : null}
              {t("tools.repairProgress", {
                current: progress?.current ?? 0,
                total: progress?.total ?? 0,
                repaired: progress?.ok ?? 0,
                skipped: progress?.skipped ?? 0,
                failed: progress?.failed ?? 0,
              })}
            </Typography.Text>
            {progress?.currentFile && (
              <Typography.Text ellipsis style={{ fontSize: 12 }} type="secondary">
                {progress.currentFile}
              </Typography.Text>
            )}
          </>
        ) : (
          <>
            <Space>
              <Button icon={<FileAddOutlined />} onClick={addByFiles}>
                {t("tools.ncmPickFiles")}
              </Button>
              <Button icon={<FolderOpenOutlined />} onClick={addByDir}>
                {t("tools.ncmPickDir")}
              </Button>
              {files.length > 0 && (
                <Typography.Text type="secondary">
                  {t("tools.fileCount", { count: files.length })}
                </Typography.Text>
              )}
            </Space>
            {files.length > 0 && (
              <>
                <List
                  size="small"
                  bordered
                  dataSource={files}
                  style={{ maxHeight: 180, overflow: "auto" }}
                  renderItem={(f) => (
                    <List.Item>
                      <Typography.Text ellipsis style={{ maxWidth: 500, fontSize: 12 }}>
                        {f}
                      </Typography.Text>
                    </List.Item>
                  )}
                />
                <Space size="large">
                  <Checkbox checked={keepSource} onChange={(e) => setKeepSource(e.target.checked)}>
                    {t("tools.ncmKeepSource")}
                  </Checkbox>
                  <Checkbox checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)}>
                    {t("tools.ncmOverwrite")}
                  </Checkbox>
                </Space>
              </>
            )}
            {control?.progress.done && (
              <Alert
                type={control.progress.canceled || failureItems.length > 0 ? "warning" : "success"}
                showIcon
                message={
                  control.progress.canceled
                    ? t("errors.ncmCanceled", {
                        0: control.progress.ok,
                        1: control.progress.skipped,
                        2: control.progress.failed,
                      })
                    : t("tools.ncmDone", {
                        converted: control.progress.ok,
                        skipped: control.progress.skipped,
                        failed: control.progress.failed,
                      })
                }
                description={
                  shownFailures.length > 0 ? (
                    <Space direction="vertical" size={2}>
                      {shownFailures.map((f, idx) => (
                        <Typography.Text key={idx} style={{ fontSize: 12 }}>
                          {f.source}：{translateItemError(f.error)}
                        </Typography.Text>
                      ))}
                      {hiddenFailures > 0 && (
                        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                          {t("tools.moreItems", { count: hiddenFailures })}
                        </Typography.Text>
                      )}
                    </Space>
                  ) : undefined
                }
              />
            )}
          </>
        )}
      </Space>
    </Modal>
  );
}
