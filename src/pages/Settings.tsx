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
  Radio,
  Select,
  Space,
  Switch,
  Typography,
  message as antMessage,
} from "antd";
import { FolderOpenOutlined } from "@ant-design/icons";
import { open } from "@tauri-apps/plugin-dialog";
import i18n, { normalizeLanguage } from "../i18n";
import { api } from "../api";
import { formatError } from "../errors";
import type { AppInfo, Config } from "../types";

const QUALITY_VALUES = ["standard", "higher", "exhigh", "lossless", "hires"] as const;

const defaultConfig: Config = {
  apiBase: "https://netease-api.muxinxy.com",
  httpProxy: null,
  musicRoot: null,
  folderTemplate: "{歌单名}",
  filenameTemplate: "{歌手} - {标题}",
  artistSeparator: "、",
  language: "zh-CN",
  ua: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
  preflight: true,
  retry: 3,
  quality: "exhigh",
  downloadSource: "auto",
  autoSyncOnStartup: true,
  syncIntervalMinutes: 60,
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
          <Divider />
          <Form.Item label={t("settings.labelApi")} name="apiBase" extra={t("settings.apiExtra")}>
            <Input />
          </Form.Item>
          <Form.Item label={t("settings.labelProxy")} name="httpProxy" extra={t("settings.proxyExtra")}>
            <Input placeholder={t("settings.placeholderProxy")} />
          </Form.Item>
        </Card>
      </Form>
    </div>
  );
}