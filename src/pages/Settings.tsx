import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Divider,
  Form,
  Input,
  InputNumber,
  List,
  Modal,
  Progress,
  Radio,
  Select,
  Space,
  Switch,
  Typography,
  message as antMessage,
} from "antd";
import { FolderOpenOutlined, FileAddOutlined, ToolOutlined } from "@ant-design/icons";
import { open } from "@tauri-apps/plugin-dialog";
import i18n, { normalizeLanguage } from "../i18n";
import { api } from "../api";
import { formatError } from "../errors";
import type { AppInfo, Config, NcmConvertItemResult, NcmConvertReport } from "../types";

const QUALITY_VALUES = ["standard", "higher", "exhigh", "lossless", "hires"] as const;

const defaultConfig: Config = {
  apiBase: "https://netease-api.muxinxy.com",
  httpProxy: null,
  musicRoot: null,
  folderTemplate: "{歌单名}",
  filenameTemplate: "{歌手} - {标题}",
  artistSeparator: "、",
  language: "zh-CN",
  theme: "system",
  ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
  preflight: true,
  retry: 3,
  quality: "exhigh",
  downloadSource: "auto",
  syncMode: "mirror",
  uploadManual: false,
  autoSyncOnStartup: false,
  syncIntervalMinutes: null,
  autoLaunch: false,
  closeToTray: true,
  useRandomCnIp: false,
  ncmConvert: true,
  ncmScanDirs: [],
  ncmKeepSource: true,
  embedCover: true,
  embedLyrics: false,
  writeLrc: true,
  writeM3u8: true,
  concurrency: 3,
  playlists: [],
};

function dirPicker(title: string) {
  return open({ directory: true, multiple: false, title }) as Promise<string | null>;
}

export default function SettingsPage() {
  const { t } = i18n;
  const [form] = Form.useForm<Config>();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [saving, setSaving] = useState(false);
  const [moving, setMoving] = useState(false);
  const [ncmToolOpen, setNcmToolOpen] = useState(false);
  const readyRef = useRef(false);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const fullConfigRef = useRef<Config>({ ...defaultConfig });

  const load = useCallback(async () => {
    try {
      const [cfg, appInfo] = await Promise.all([api.getConfig(), api.getAppInfo()]);
      const merged = { ...defaultConfig, ...cfg };
      fullConfigRef.current = merged;
      form.setFieldsValue(merged);
      setInfo(appInfo);
      readyRef.current = true;
    } catch (e) {
      antMessage.error(t("settings.loadFailed", { detail: formatError(e) }));
    }
  }, [form, t]);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(
    () => () => {
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    },
    []
  );

  const save = useCallback(
    async (values?: Partial<Config>) => {
      setSaving(true);
      try {
        const merged = { ...fullConfigRef.current, ...values };
        fullConfigRef.current = merged;
        await api.saveConfig(merged);
        antMessage.success(t("settings.saved"));
      } catch (e) {
        antMessage.error(t("settings.saveFailed", { detail: formatError(e) }));
      } finally {
        setSaving(false);
      }
    },
    [t]
  );

  const scheduleAutoSave = useCallback(
    (_changed: unknown, all: Partial<Config>) => {
      if (!readyRef.current || moving) return;
      if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
      saveTimerRef.current = setTimeout(() => save(all), 500);
    },
    [moving, save]
  );

  const applyLanguage = useCallback(
    async (language: string) => {
      const normalized = normalizeLanguage(language);
      i18n.changeLanguage(normalized);
      await api.setLanguage(normalized);
    },
    []
  );

  const onLanguageChange = async (language: string) => {
    form.setFieldValue("language", language);
    if (!readyRef.current || moving) return;
    await applyLanguage(language);
    await save(form.getFieldsValue(true));
  };

  const chooseMusicRoot = async () => {
    try {
      const path = await dirPicker(t("settings.pickRootTitle"));
      if (!path) {
        antMessage.info(t("settings.canceledPick"));
        return;
      }
      form.setFieldValue("musicRoot", path);
      if (readyRef.current && !moving) save(form.getFieldsValue(true));
    } catch (e) {
      antMessage.error(t("settings.pickFailed", { detail: formatError(e) }));
    }
  };

  const chooseDataDir = async () => {
    const path = await dirPicker(t("settings.pickDataTitle"));
    if (!path || path === info?.dataDir) return;
    setMoving(true);
    try {
      const next = await api.setDataDir(path, true);
      setInfo(next);
      antMessage.success(t("settings.moved"));
    } catch (e) {
      antMessage.error(t("settings.moveFailed", { detail: formatError(e) }));
    } finally {
      setMoving(false);
    }
  };

  return (
    <div style={{ padding: 24, maxWidth: 920 }}>
      <Form
        form={form}
        layout="vertical"
        initialValues={defaultConfig}
        onValuesChange={scheduleAutoSave}
      >
        <Card title={t("settings.cardData")} style={{ marginBottom: 16 }}>
          <Alert
            type={info?.dataDirPortable ? "success" : "info"}
            showIcon
            message={info?.dataDirPortable ? t("settings.dataPortable") : t("settings.dataApp")}
            description={t("settings.dataDesc")}
            style={{ marginBottom: 16 }}
          />
          <Space.Compact style={{ width: "100%" }}>
            <Input value={info?.dataDir ?? t("settings.placeholderData")} readOnly />
            <Button loading={moving} icon={<FolderOpenOutlined />} onClick={chooseDataDir}>
              {t("settings.changeData")}
            </Button>
          </Space.Compact>
        </Card>

        <Card title={t("settings.cardStorage")} style={{ marginBottom: 16 }}>
          <Form.Item label={t("settings.labelRoot")} name="musicRoot" extra={t("settings.rootExtra")}>
            <Input
              readOnly
              placeholder={t("settings.placeholderRoot")}
              onClick={chooseMusicRoot}
              suffix={
                <Button
                  size="small"
                  type="text"
                  icon={<FolderOpenOutlined />}
                  onClick={chooseMusicRoot}
                >
                  {t("settings.choose")}
                </Button>
              }
            />
          </Form.Item>
          <Form.Item
            label={t("settings.labelFolderTemplate")}
            name="folderTemplate"
            extra={t("settings.folderTemplateExtra")}
          >
            <Input />
          </Form.Item>
          <Form.Item
            label={t("settings.labelFilenameTemplate")}
            name="filenameTemplate"
            extra={t("settings.filenameTemplateExtra")}
          >
            <Input />
          </Form.Item>
          <Form.Item
            label={t("settings.labelSeparator")}
            name="artistSeparator"
            tooltip={t("settings.separatorTip")}
          >
            <Input style={{ width: 160 }} placeholder="、" />
          </Form.Item>
          <Form.Item name="writeM3u8" valuePropName="checked">
            <Checkbox>{t("settings.cbM3u8")}</Checkbox>
          </Form.Item>
        </Card>

        <Card title={t("settings.cardDownload")} style={{ marginBottom: 16 }}>
          <Form.Item label={t("settings.labelQuality")} name="quality">
            <Radio.Group>
              {QUALITY_VALUES.map((value) => (
                <Radio.Button key={value} value={value}>
                  {t(`playlists.qualityOptions.${value}`)}
                </Radio.Button>
              ))}
            </Radio.Group>
          </Form.Item>
          <Form.Item label={t("settings.labelDownloadSource")} name="downloadSource" extra={t("settings.downloadSourceExtra")}>
            <Select
              style={{ width: 240 }}
              options={[
                { value: "auto", label: t("settings.downloadSourceAuto") },
                { value: "download", label: t("settings.downloadSourceDownload") },
              ]}
            />
          </Form.Item>
          <Form.Item label={t("settings.labelConcurrency")} name="concurrency">
            <InputNumber min={1} max={5} />
          </Form.Item>
          <Form.Item label={t("settings.labelRetry")} name="retry">
            <InputNumber min={0} max={5} />
          </Form.Item>
          <Form.Item name="preflight" valuePropName="checked" label={t("settings.cbPreflight")}>
            <Switch checkedChildren={t("settings.on")} unCheckedChildren={t("settings.off")} />
          </Form.Item>
          <Form.Item label={t("settings.labelUa")} name="ua" tooltip={t("settings.uaTip")}>
            <Input />
          </Form.Item>
          <Form.Item name="ncmConvert" valuePropName="checked">
            <Checkbox>{t("settings.cbNcm")}</Checkbox>
          </Form.Item>
          <Form.Item name="ncmKeepSource" valuePropName="checked">
            <Checkbox>{t("settings.cbNcmKeep")}</Checkbox>
          </Form.Item>
          <Form.Item>
            <Button icon={<ToolOutlined />} onClick={() => setNcmToolOpen(true)}>
              {t("settings.ncmToolOpen")}
            </Button>
          </Form.Item>
          <Form.Item name="embedCover" valuePropName="checked">
            <Checkbox>{t("settings.cbCover")}</Checkbox>
          </Form.Item>
          <Form.Item name="embedLyrics" valuePropName="checked">
            <Checkbox>{t("settings.cbLyrics")}</Checkbox>
          </Form.Item>
          <Form.Item name="writeLrc" valuePropName="checked">
            <Checkbox>{t("settings.cbLrc")}</Checkbox>
          </Form.Item>
        </Card>

        <Card title={t("settings.cardAuto")} style={{ marginBottom: 16 }}>
          <Form.Item name="autoSyncOnStartup" valuePropName="checked" label={t("settings.labelAutoSync")}>
            <Switch checkedChildren={t("settings.on")} unCheckedChildren={t("settings.off")} />
          </Form.Item>
          <Form.Item name="autoLaunch" valuePropName="checked" label={t("settings.labelAutoLaunch")} extra={t("settings.autoLaunchExtra")}>
            <Switch
              checkedChildren={t("settings.on")}
              unCheckedChildren={t("settings.off")}
              onChange={async (checked) => {
                try {
                  await api.setAutoLaunch(checked);
                } catch (e) {
                  antMessage.error(t("settings.autoLaunchFailed", { detail: formatError(e) }));
                }
              }}
            />
          </Form.Item>
          <Form.Item label={t("settings.labelInterval")} name="syncIntervalMinutes">
            <InputNumber min={15} max={10080} style={{ width: 160 }} />
          </Form.Item>
          <Form.Item name="closeToTray" valuePropName="checked" label={t("settings.cbCloseToTray")}>
            <Switch checkedChildren={t("settings.on")} unCheckedChildren={t("settings.off")} />
          </Form.Item>
          <Form.Item name="useRandomCnIp" valuePropName="checked" label={t("settings.cbRandomCnIp")}>
            <Switch checkedChildren={t("settings.on")} unCheckedChildren={t("settings.off")} />
          </Form.Item>
          <Form.Item label={t("settings.labelLanguage")} name="language">
            <Select
              style={{ width: 200 }}
              options={[
                { value: "zh-CN", label: t("settings.langZh") },
                { value: "en", label: t("settings.langEn") },
              ]}
              onChange={onLanguageChange}
            />
          </Form.Item>
          <Form.Item label={t("settings.labelTheme")} name="theme">
            <Select
              style={{ width: 200 }}
              onChange={async (value) => {
                window.dispatchEvent(new Event("theme-changed"));
              }}
              options={[
                { value: "system", label: t("settings.themeSystem") },
                { value: "light", label: t("settings.themeLight") },
                { value: "dark", label: t("settings.themeDark") },
              ]}
            />
          </Form.Item>
          <Divider />
          <Form.Item label={t("settings.labelApi")} name="apiBase" extra={t("settings.apiExtra")}>
            <Input />
          </Form.Item>
          <Form.Item label={t("settings.labelProxy")} name="httpProxy" extra={t("settings.proxyExtra")}>
            <Input placeholder={t("settings.placeholderProxy")} />
          </Form.Item>
        </Card>

        <Card title={t("settings.cardSyncMode")} style={{ marginBottom: 16 }}>
          <Form.Item label={t("settings.labelMode")} name="syncMode" extra={t("settings.modeExtra")}>
            <Radio.Group>
              <Radio.Button value="mirror">{t("settings.modeMirror")}</Radio.Button>
              <Radio.Button value="add_only">{t("settings.modeAddOnly")}</Radio.Button>
              <Radio.Button value="delete_only">{t("settings.modeDeleteOnly")}</Radio.Button>
            </Radio.Group>
          </Form.Item>
          <Form.Item
            name="uploadManual"
            valuePropName="checked"
            label={t("settings.labelUploadManual")}
            extra={t("settings.uploadManualExtra")}
          >
            <Switch checkedChildren={t("settings.on")} unCheckedChildren={t("settings.off")} />
          </Form.Item>
          <Alert type="info" showIcon message={t("settings.syncModeHint")} />
        </Card>

        <NcmToolModal open={ncmToolOpen} onClose={() => setNcmToolOpen(false)} />
      </Form>
    </div>
  );
}

/** 独立 NCM 转换工具弹窗：选文件（可多选）或目录 → 列出 .ncm → 转换。 */
function NcmToolModal({ open: isOpen, onClose }: { open: boolean; onClose: () => void }) {
  const { t } = i18n;
  const [files, setFiles] = useState<string[]>([]);
  const [keepSource, setKeepSource] = useState(true);
  const [overwrite, setOverwrite] = useState(false);
  const [running, setRunning] = useState(false);
  const [done, setDone] = useState<NcmConvertReport | null>(null);
  const [progress, setProgress] = useState<number>(0);

  const reset = () => {
    setFiles([]);
    setDone(null);
    setProgress(0);
  };

  const close = () => {
    if (running) return;
    reset();
    onClose();
  };

  const addByFiles = async () => {
    const picked = (await open({
      multiple: true,
      filters: [{ name: "NCM", extensions: ["ncm"] }],
      title: t("settings.ncmPickFiles"),
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
    const dir = (await dirPicker(t("settings.ncmPickDir"))) as string | null;
    if (!dir) return;
    setFiles((prev) => (prev.includes(dir) ? prev : [...prev, dir]));
  };

  const start = async () => {
    if (files.length === 0 || running) return;
    setRunning(true);
    setDone(null);
    try {
      const report = await api.convertNcmManual(files, keepSource, overwrite);
      setDone(report);
      const total = report.converted + report.skipped + report.failed;
      setProgress(total > 0 ? Math.round(((report.converted + report.skipped) / total) * 100) : 100);
      antMessage.success(
        t("settings.ncmToolDone", {
          converted: report.converted,
          skipped: report.skipped,
          failed: report.failed,
        })
      );
    } catch (e) {
      antMessage.error(formatError(e));
    } finally {
      setRunning(false);
    }
  };

  const failureItems = done?.items.filter((i) => i.status === "failed") ?? [];

  return (
    <Modal
      title={t("settings.ncmToolTitle")}
      open={isOpen}
      onCancel={close}
      onOk={start}
      okText={t("settings.ncmToolStart")}
      cancelText={t("settings.cancel")}
      confirmLoading={running}
      okButtonProps={{ disabled: files.length === 0 || running }}
      width={640}
    >
      <Space direction="vertical" style={{ width: "100%" }} size="middle">
        <Space>
          <Button icon={<FileAddOutlined />} onClick={addByFiles} disabled={running}>
            {t("settings.ncmPickFiles")}
          </Button>
          <Button icon={<FolderOpenOutlined />} onClick={addByDir} disabled={running}>
            {t("settings.ncmPickDir")}
          </Button>
          {files.length > 0 && (
            <Typography.Text type="secondary">
              {t("settings.ncmFileCount", { count: files.length })}
            </Typography.Text>
          )}
        </Space>
        {files.length > 0 && (
          <>
            <List
              size="small"
              bordered
              dataSource={files}
              style={{ maxHeight: 200, overflow: "auto" }}
              renderItem={(f) => (
                <List.Item>
                  <Typography.Text ellipsis style={{ maxWidth: 520, fontSize: 12 }}>
                    {f}
                  </Typography.Text>
                </List.Item>
              )}
            />
            <Space size="large">
              <Checkbox checked={keepSource} onChange={(e) => setKeepSource(e.target.checked)}>
                {t("settings.ncmKeepSource")}
              </Checkbox>
              <Checkbox checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)}>
                {t("settings.ncmOverwrite")}
              </Checkbox>
            </Space>
            {running && <Progress percent={progress} status="active" />}
            {done && (
              <Alert
                type={done.failed > 0 ? "warning" : "success"}
                showIcon
                message={t("settings.ncmToolDone", {
                  converted: done.converted,
                  skipped: done.skipped,
                  failed: done.failed,
                })}
                description={
                  failureItems.length > 0 ? (
                    <Space direction="vertical" size={2}>
                      {failureItems.slice(0, 5).map((f: NcmConvertItemResult, idx: number) => (
                        <Typography.Text key={idx} style={{ fontSize: 12 }}>
                          {f.source}：{f.error}
                        </Typography.Text>
                      ))}
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