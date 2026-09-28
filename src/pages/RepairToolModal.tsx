import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Checkbox,
  Input,
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

const KIND = "repair";

/** 文件属性修复工具弹窗：选文件/目录 → 选择修复项 → 后台任务执行，可关闭窗口继续。 */
export default function RepairToolModal({ open: isOpen, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = i18n;
  const [paths, setPaths] = useState<string[]>([]);
  const [fixTags, setFixTags] = useState(true);
  const [fixCover, setFixCover] = useState(true);
  const [fixLyrics, setFixLyrics] = useState(false);
  const [fixFilename, setFixFilename] = useState(false);
  const [template, setTemplate] = useState("");
  const [running, setRunning] = useState(false);
  const [starting, setStarting] = useState(false);
  const [control, setControl] = useState<ToolControl | null>(null);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const startingRef = useRef(false);
  const resultsShownRef = useRef(false);

  // 打开时拉取任务状态（含后台仍在跑的任务）与设置里的文件名模板。
  useEffect(() => {
    if (!isOpen) return;
    api
      .getConfig()
      .then((cfg) => setTemplate(cfg.filenameTemplate ?? "{歌手} - {标题}"))
      .catch(() => {});
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

  // 运行中轮询进度；结束后拉取最终快照（含条目明细）。
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
        // 轮询失败忽略，下个周期重试
      }
    };
    poll();
    pollRef.current = setInterval(poll, 800);
    return stopPolling;
  }, [running, stopPolling]);

  const addPaths = async (byDir: boolean) => {
    const picked = byDir
      ? ((await open({ multiple: false, directory: true, title: t("tools.repairPickDir") })) as
          | string
          | string[]
          | null)
      : ((await open({
          multiple: true,
          filters: [{ name: "Audio", extensions: ["mp3", "flac", "m4a", "wav", "ogg", "aac"] }],
          title: t("tools.repairPickFiles"),
        })) as string[] | string | null);
    if (!picked) return;
    const list = Array.isArray(picked) ? picked : [picked];
    setPaths((prev) => {
      const next = [...prev];
      for (const p of list) if (!next.includes(p)) next.push(p);
      return next;
    });
  };

  const removePath = (p: string) => setPaths((prev) => prev.filter((x) => x !== p));

  const start = async () => {
    if (startingRef.current || running) return;
    if (paths.length === 0) {
      antMessage.warning(t("errors.repairNoFiles"));
      return;
    }
    startingRef.current = true;
    setStarting(true);
    try {
      await api.startRepair(paths, fixTags, fixCover, fixLyrics, fixFilename, template.trim() || null);
      setRunning(true);
      setPaths([]);
      resultsShownRef.current = true;
      antMessage.success(t("tools.repairStarted"));
    } catch (e) {
      antMessage.error(formatError(e));
    } finally {
      startingRef.current = false;
      setStarting(false);
    }
  };

  const cancel = async () => {
    try {
      await api.cancelTool(KIND);
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

  const progress = control?.progress;
  const paused = progress?.paused ?? false;
  const items = control?.items ?? [];
  const skippedItems = items.filter((i) => i.status === "skipped").slice(0, 6);
  const failureItems = items.filter((i) => i.status === "failed").slice(0, 6);
  const hiddenItems = items.length - skippedItems.length - failureItems.length;

  return (
    <Modal
      title={t("tools.repairTitle")}
      open={isOpen}
      onCancel={() => onClose()}
      footer={
        running
          ? [
              <Button key="pause" onClick={pauseResume}>
                {paused ? t("app.resume") : t("app.pause")}
              </Button>,
              <Button key="cancel-task" danger onClick={cancel}>
                {t("tools.repairCancel")}
              </Button>,
              <Button key="close" type="primary" onClick={() => onClose()}>
                {t("tools.close")}
              </Button>,
            ]
          : [
              <Button key="close" onClick={() => onClose()}>
                {t("tools.close")}
              </Button>,
              <Button
                key="start"
                type="primary"
                loading={starting}
                onClick={start}
              >
                {t("tools.repairStart")}
              </Button>,
            ]
      }
      width={680}
    >
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        {running ? (
          <>
            <Alert type="info" showIcon message={t("tools.repairBackgroundHint")} />
            <Progress
              percent={progress && progress.total > 0 ? Math.round((progress.current / progress.total) * 100) : 0}
              status={
                progress?.canceled
                  ? "exception"
                  : progress?.done
                    ? "success"
                    : paused
                      ? "normal"
                      : "active"
              }
            />
            <Typography.Text type="secondary" style={{ fontSize: 12 }}>
              {paused ? <Typography.Text type="warning">{t("tools.taskPaused")} · </Typography.Text> : null}
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
              <Button icon={<FileAddOutlined />} onClick={() => addPaths(false)} disabled={starting}>
                {t("tools.repairPickFiles")}
              </Button>
              <Button icon={<FolderOpenOutlined />} onClick={() => addPaths(true)} disabled={starting}>
                {t("tools.repairPickDir")}
              </Button>
              {paths.length > 0 && (
                <Typography.Text type="secondary">
                  {t("tools.fileCount", { count: paths.length })}
                </Typography.Text>
              )}
            </Space>
            {paths.length > 0 && (
              <List
                size="small"
                bordered
                dataSource={paths}
                style={{ maxHeight: 160, overflow: "auto" }}
                renderItem={(p) => (
                  <List.Item
                    actions={[
                      <Button
                        key="remove"
                        type="text"
                        size="small"
                        danger
                        disabled={starting}
                        onClick={() => removePath(p)}
                      >
                        {t("tools.remove")}
                      </Button>,
                    ]}
                  >
                    <Typography.Text ellipsis style={{ maxWidth: 460, fontSize: 12 }}>
                      {p}
                    </Typography.Text>
                  </List.Item>
                )}
              />
            )}
            <Space direction="vertical" size={4}>
              <Checkbox checked={fixTags} onChange={(e) => setFixTags(e.target.checked)} disabled={starting}>
                {t("tools.repairFixTags")}
              </Checkbox>
              <Checkbox checked={fixCover} onChange={(e) => setFixCover(e.target.checked)} disabled={starting}>
                {t("tools.repairFixCover")}
              </Checkbox>
              <Checkbox checked={fixLyrics} onChange={(e) => setFixLyrics(e.target.checked)} disabled={starting}>
                {t("tools.repairFixLyrics")}
              </Checkbox>
              <Checkbox checked={fixFilename} onChange={(e) => setFixFilename(e.target.checked)} disabled={starting}>
                {t("tools.repairFixFilename")}
              </Checkbox>
            </Space>
            {fixFilename && (
              <Space direction="vertical" size={2} style={{ width: "100%" }}>
                <Typography.Text style={{ fontSize: 12 }}>{t("tools.repairTemplateLabel")}</Typography.Text>
                <Input
                  value={template}
                  onChange={(e) => setTemplate(e.target.value)}
                  placeholder="{歌手} - {标题}"
                />
                <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                  {t("tools.repairTemplateExtra")}
                </Typography.Text>
              </Space>
            )}
          </>
        )}
        {!running && resultsShownRef.current && control?.progress.done && (
          <Alert
            type={control.progress.canceled || failureItems.length > 0 ? "warning" : "success"}
            showIcon
            message={
              control.progress.canceled
                ? t("errors.repairCanceled", {
                    0: control.progress.ok,
                    1: control.progress.skipped,
                    2: control.progress.failed,
                  })
                : t("tools.repairDone", {
                    repaired: control.progress.ok,
                    skipped: control.progress.skipped,
                    failed: control.progress.failed,
                  })
            }
            description={
              skippedItems.length + failureItems.length > 0 ? (
                <Space direction="vertical" size={2}>
                  {skippedItems.map((f: RepairItemResult, idx: number) => (
                    <Typography.Text key={`s${idx}`} style={{ fontSize: 12 }} type="secondary">
                      {f.source}：{translateItemError(f.error) || t("tools.repairUnidentified")}
                    </Typography.Text>
                  ))}
                  {failureItems.map((f: RepairItemResult, idx: number) => (
                    <Typography.Text key={`f${idx}`} style={{ fontSize: 12 }}>
                      {f.source}：{translateItemError(f.error)}
                    </Typography.Text>
                  ))}
                  {hiddenItems > 0 && (
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      {t("tools.moreItems", { count: hiddenItems })}
                    </Typography.Text>
                  )}
                </Space>
              ) : undefined
            }
          />
        )}
      </Space>
    </Modal>
  );
}
