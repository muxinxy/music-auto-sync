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
import { api } from "../api";
import type { AppInfo, Config } from "../types";

const defaultConfig: Config = {
  apiBase: "https://netease-api.muxinxy.com",
  httpProxy: null,
  musicRoot: null,
  folderTemplate: "{歌单名}",
  filenameTemplate: "{歌手} - {标题}",
  quality: "exhigh",
  autoSyncOnStartup: true,
  syncIntervalMinutes: 60,
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
  const [form] = Form.useForm<Config>();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [saving, setSaving] = useState(false);
  const [moving, setMoving] = useState(false);
  const readyRef = useRef(false);
  const saveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 完整配置引用：包含 cookie、cookieUser、playlists 等不在表单里的字段，
  // 自动保存时与表单值合并，避免把登录凭据和歌单配置清空。
  const fullConfigRef = useRef<Config>({ ...defaultConfig });

  const load = useCallback(async () => {
    try {
      const [cfg, appInfo] = await Promise.all([api.getConfig(), api.getAppInfo()]);
      const merged = { ...defaultConfig, ...cfg };
      fullConfigRef.current = merged;
      form.setFieldsValue(merged);
      setInfo(appInfo);
      readyRef.current = true;
    } catch (e) { antMessage.error(`加载设置失败：${e}`); }
  }, [form]);

  useEffect(() => { load(); }, [load]);

  useEffect(() => () => {
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
  }, []);

  const save = async (values?: Partial<Config>) => {
    setSaving(true);
    try {
      const merged = { ...fullConfigRef.current, ...values };
      fullConfigRef.current = merged;
      await api.saveConfig(merged);
      antMessage.success("设置已自动保存");
    } catch (e) { antMessage.error(`保存失败：${e}`); }
    finally { setSaving(false); }
  };

  const scheduleAutoSave = (_changed: unknown, all: Partial<Config>) => {
    if (!readyRef.current || moving) return;
    if (saveTimerRef.current) clearTimeout(saveTimerRef.current);
    saveTimerRef.current = setTimeout(() => save(all), 500);
  };

  const chooseMusicRoot = async () => {
    try {
      const path = await dirPicker("选择音乐根目录");
      if (!path) {
        antMessage.info("已取消选择目录");
        return;
      }
      form.setFieldValue("musicRoot", path);
      if (readyRef.current && !moving) save(form.getFieldsValue(true));
    } catch (e) {
      antMessage.error(`选择音乐根目录失败：${e}`);
    }
  };

  const chooseDataDir = async () => {
    const path = await dirPicker("选择应用数据目录");
    if (!path || path === info?.dataDir) return;
    setMoving(true);
    try {
      const next = await api.setDataDir(path, true);
      setInfo(next);
      antMessage.success("数据已迁移到新目录");
    } catch (e) { antMessage.error(`迁移失败：${e}`); }
    finally { setMoving(false); }
  };

  return (
    <div style={{ padding: 24, maxWidth: 920 }}>
      <Form form={form} layout="vertical" initialValues={defaultConfig} onValuesChange={scheduleAutoSave}>
        <Card title="便携数据目录" style={{ marginBottom: 16 }}>
          <Alert
            type={info?.dataDirPortable ? "success" : "info"}
            showIcon
            message={info?.dataDirPortable ? "便携模式已启用" : "当前使用系统应用数据目录"}
            description="配置、登录凭据、数据库、日志和下载缓存都保存在此目录。便携版可直接将应用文件夹整体复制到其它电脑。所有设置修改会自动保存。"
            style={{ marginBottom: 16 }}
          />
          <Space.Compact style={{ width: "100%" }}>
            <Input value={info?.dataDir ?? "正在读取…"} readOnly />
            <Button loading={moving} icon={<FolderOpenOutlined />} onClick={chooseDataDir}>
              更改并迁移
            </Button>
          </Space.Compact>
        </Card>

        <Card title="存储与命名" style={{ marginBottom: 16 }}>
          <Form.Item label="音乐根目录" name="musicRoot" extra="所有下载、歌单文件夹和 .quarantine 隔离目录都在此处。">
            <Input
              readOnly
              placeholder="请选择音乐根目录"
              onClick={chooseMusicRoot}
              suffix={
                <Button
                  size="small"
                  type="text"
                  icon={<FolderOpenOutlined />}
                  onClick={chooseMusicRoot}
                >
                  选择
                </Button>
              }
            />
          </Form.Item>
          <Form.Item label="歌单文件夹模板" name="folderTemplate" extra="可用变量：{歌单名}、{歌手}、{专辑}">
            <Input />
          </Form.Item>
          <Form.Item label="文件名模板" name="filenameTemplate" extra="可用变量：{音轨号}、{歌手}、{标题}、{专辑}、{网易云ID}">
            <Input />
          </Form.Item>
          <Form.Item name="writeM3u8" valuePropName="checked">
            <Checkbox>为每个歌单生成 playlist.m3u8</Checkbox>
          </Form.Item>
        </Card>

        <Card title="下载与元数据" style={{ marginBottom: 16 }}>
          <Form.Item label="默认音质" name="quality">
            <Radio.Group>
              <Radio.Button value="standard">标准</Radio.Button>
              <Radio.Button value="higher">较高</Radio.Button>
              <Radio.Button value="exhigh">极高 320k</Radio.Button>
              <Radio.Button value="lossless">无损 FLAC</Radio.Button>
              <Radio.Button value="hires">Hi-Res</Radio.Button>
            </Radio.Group>
          </Form.Item>
          <Form.Item label="最大下载并发数" name="concurrency">
            <InputNumber min={1} max={5} />
          </Form.Item>
          <Form.Item name="ncmConvert" valuePropName="checked">
            <Checkbox>检测到 .ncm 文件时自动转换为 mp3 / flac</Checkbox>
          </Form.Item>
          <Form.Item name="ncmKeepSource" valuePropName="checked">
            <Checkbox>转换后保留原始 .ncm 文件（取消勾选则转换成功后删除源文件）</Checkbox>
          </Form.Item>
          <Form.Item name="embedCover" valuePropName="checked">
            <Checkbox>写入专辑封面</Checkbox>
          </Form.Item>
          <Form.Item name="embedLyrics" valuePropName="checked">
            <Checkbox>嵌入歌词标签</Checkbox>
          </Form.Item>
          <Form.Item name="writeLrc" valuePropName="checked">
            <Checkbox>保存同名 .lrc 歌词文件</Checkbox>
          </Form.Item>
        </Card>

        <Card title="自动同步与服务" style={{ marginBottom: 16 }}>
          <Form.Item name="autoSyncOnStartup" valuePropName="checked" label="启动后自动同步">
            <Switch checkedChildren="开启" unCheckedChildren="关闭" />
          </Form.Item>
          <Form.Item label="定时轮询间隔（分钟，留空则关闭）" name="syncIntervalMinutes">
            <InputNumber min={15} max={10080} style={{ width: 160 }} />
          </Form.Item>
          <Divider />
          <Form.Item label="网易云 API 地址" name="apiBase" extra="使用兼容 NeteaseCloudMusicApi Enhanced 的服务器。公共实例可能因网络或访问策略返回 403。">
            <Input />
          </Form.Item>
          <Form.Item label="HTTP(S) 代理地址" name="httpProxy" extra="仅用于访问已配置 API 服务；留空为直连。例如 http://127.0.0.1:7897。">
            <Input placeholder="http://127.0.0.1:7897" />
          </Form.Item>
        </Card>
      </Form>
    </div>
  );
}
