import { useCallback, useEffect, useState, useSyncExternalStore } from "react";
import { Alert, App as AntApp, Avatar, Button, Layout, Menu, Popconfirm, Select, Space, Tag, theme, Typography } from "antd";
import {
  CloudSyncOutlined,
  CloudUploadOutlined,
  DeleteOutlined,
  HistoryOutlined,
  InfoCircleOutlined,
  LoginOutlined,
  ReloadOutlined,
  SettingOutlined,
} from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import i18n, { normalizeLanguage } from "./i18n";
import { api } from "./api";
import type { CloudTaskRow, LoginStatus, SyncProgress, UiMessage } from "./types";
import { translateUi } from "./errors";
import { syncStore } from "./syncStore";
import { cloudStore } from "./cloudStore";
import { taskDisplayName } from "./taskName";
import LoginPage from "./pages/Login";
import PlaylistsPage from "./pages/Playlists";
import CloudPage from "./pages/Cloud";
import SyncPage from "./pages/Sync";
import QuarantinePage from "./pages/Quarantine";
import SettingsPage from "./pages/Settings";
import AboutPage from "./pages/About";

const { Sider, Content, Header } = Layout;

export type PageKey =
  | "login"
  | "playlists"
  | "cloud"
  | "sync"
  | "quarantine"
  | "settings"
  | "about";

export interface SyncEventState {
  running: boolean;
  paused?: boolean;
  progress?: SyncProgress;
}

export default function App() {
  const { message } = AntApp.useApp();
  const { t } = useTranslation();
  const { token } = theme.useToken();
  const [page, setPage] = useState<PageKey>("login");
  const [login, setLogin] = useState<LoginStatus | null>(null);
  const [appReady, setAppReady] = useState(false);
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [prefLanguage, setPrefLanguage] = useState<string>(i18n.language);
  const [prefTheme, setPrefTheme] = useState<string>("system");

  // 低频运行状态（running/paused）：来自进度外部 store，不随每曲目 progress 重渲染。
  const syncRunning = useSyncExternalStore(syncStore.subscribeRunning, syncStore.getRunning);
  const syncPaused = useSyncExternalStore(syncStore.subscribeRunning, syncStore.getPaused);
  const cloudRunning = useSyncExternalStore(cloudStore.subscribeRunning, cloudStore.getRunning);
  const sync: SyncEventState = { running: syncRunning, paused: syncPaused };
  const taskRunning = syncRunning || cloudRunning;

  const applyLanguage = useCallback(async () => {
    try {
      const config = await api.getConfig();
      i18n.changeLanguage(normalizeLanguage(config.language));
      // 同步托盘与窗口标题
      await api.setLanguage(config.language ?? "zh-CN");
    } catch {
      // 语言加载失败不阻塞启动
    }
  }, []);

  const refreshLogin = useCallback(async (verifyAttempt?: number, retryLimit?: number) => {
    try {
      const s = await api.getLoginStatus(verifyAttempt, retryLimit);
      setLogin(s);
      return s;
    } catch {
      return null;
    }
  }, []);

  useEffect(() => {
    applyLanguage();
    // 同步 Header 快捷切换显示值（语言/主题）。
    api.getConfig().then((cfg) => {
      setPrefLanguage(normalizeLanguage(cfg.language));
      setPrefTheme(cfg.theme ?? "system");
    }).catch(() => {});
    const onThemeChanged = () => {
      api.getConfig().then((cfg) => setPrefTheme(cfg.theme ?? "system")).catch(() => {});
    };
    window.addEventListener("theme-changed", onThemeChanged);
    // 启动始终停留在“账号登录”页（登录后该页显示账号信息与统计）；
    // 已登录也由用户自行点击左侧菜单进入歌单页。
    refreshLogin().finally(() => setAppReady(true));

    api.checkForUpdate().then((version) => {
      if (version) setUpdateVersion(version);
    }).catch(() => {});

    const unlistenProgress = listen<SyncProgress>("sync://progress", (e) => {
      syncStore.setProgress(e.payload);
    });
    const unlistenState = listen<boolean>("sync://state", (e) => {
      syncStore.setRunning(e.payload, e.payload ? syncStore.getPaused() : false);
    });
    // 云盘任务：独立于歌单同步并行运行，走 cloud:// 事件通道。
    // start 携带本轮 run id：日志详情对最近一次云盘任务直接复用内存明细。
    const unlistenCloudStart = listen<number>("cloud://start", (e) => {
      cloudStore.start(e.payload);
    });
    const unlistenCloudRow = listen<CloudTaskRow>("cloud://row", (e) => {
      cloudStore.upsert(e.payload);
    });
    const unlistenCloudState = listen<boolean>("cloud://state", (e) => {
      cloudStore.setRunning(e.payload, e.payload ? cloudStore.getPaused() : false);
    });
    const unlistenCloudProgress = listen<SyncProgress>("cloud://progress", (e) => {
      cloudStore.setProgress(e.payload);
    });
    // 轮询同步控制状态（暂停/继续），保持 UI 与后端一致。
    // 任一任务（歌单/云盘）运行期间轮询；空闲时停表，避免常驻每 1 秒一次的 IPC 调用。
    let poll: ReturnType<typeof setInterval> | null = null;
    const syncFromControl = async () => {
      try {
        const ctrl = await api.getSyncControl();
        syncStore.setRunning(ctrl.running, ctrl.running ? ctrl.paused : false);
      } catch {
        // 忽略轮询失败
      }
      if (cloudStore.getRunning()) {
        try {
          const ctrl = await api.getCloudControl();
          cloudStore.setRunning(ctrl.running, ctrl.running ? ctrl.paused : false);
        } catch {
          // 忽略轮询失败
        }
      }
    };
    const ensurePolling = () => {
      const anyRunning = syncStore.getRunning() || cloudStore.getRunning();
      if (anyRunning && !poll) {
        poll = setInterval(syncFromControl, 1000);
      } else if (!anyRunning && poll) {
        clearInterval(poll);
        poll = null;
      }
    };
    // 运行状态变化时决定启停轮询。
    const unsubRunning = syncStore.subscribeRunning(() => {
      ensurePolling();
      if (!syncStore.getRunning()) refreshLogin();
    });
    const unsubCloudRunning = cloudStore.subscribeRunning(ensurePolling);
    ensurePolling();

    return () => {
      unlistenProgress.then((f) => f());
      unlistenState.then((f) => f());
      unlistenCloudStart.then((f) => f());
      unlistenCloudRow.then((f) => f());
      unlistenCloudState.then((f) => f());
      unlistenCloudProgress.then((f) => f());
      window.removeEventListener("theme-changed", onThemeChanged);
      unsubRunning();
      unsubCloudRunning();
      if (poll) clearInterval(poll);
    };
  }, [applyLanguage, refreshLogin]);

  const onLogout = () => {
    setLogin(null);
    message.info(t("app.loggedOut"));
    setPage("login");
  };

  /** Header 快捷切语言：先落盘再即时生效。 */
  const changeLanguagePref = async (language: string) => {
    const normalized = normalizeLanguage(language);
    setPrefLanguage(normalized);
    i18n.changeLanguage(normalized);
    try {
      const cfg = await api.getConfig();
      await api.saveConfig({ ...cfg, language: normalized });
    } catch {
      // 保存失败不阻塞已生效的语言切换
    }
    await api.setLanguage(normalized).catch(() => {});
  };

  /** Header 快捷切主题：先落盘再派发事件，确保 main.tsx 读到新值即时生效。 */
  const changeThemePref = async (pref: string) => {
    setPrefTheme(pref);
    try {
      const cfg = await api.getConfig();
      await api.saveConfig({ ...cfg, theme: pref });
      window.dispatchEvent(new Event("theme-changed"));
    } catch {
      // 保存失败不派发（主题保持原样）
    }
  };

  const items = [
    { key: "login", icon: <LoginOutlined />, label: t("app.menu.login") },
    { key: "playlists", icon: <CloudSyncOutlined />, label: t("app.menu.playlists") },
    { key: "cloud", icon: <CloudUploadOutlined />, label: t("app.menu.cloud") },
    { key: "sync", icon: <HistoryOutlined />, label: t("app.menu.sync") },
    { key: "quarantine", icon: <DeleteOutlined />, label: t("app.menu.quarantine") },
    { key: "settings", icon: <SettingOutlined />, label: t("app.menu.settings") },
    { key: "about", icon: <InfoCircleOutlined />, label: t("app.menu.about") },
  ];

  return (
    <Layout style={{ height: "100vh" }}>
      <Sider theme="dark" width={200}>
        <div style={{ padding: "20px 16px", color: "#fff" }}>
          <ReloadOutlined style={{ color: "#c20c0c", marginRight: 8 }} />
          <span style={{ fontWeight: 600 }}>{t("app.brand")}</span>
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[page]}
          items={items}
          onClick={(e) => setPage(e.key as PageKey)}
        />
        <div style={{ position: "absolute", bottom: 12, left: 16, color: "#888", fontSize: 12 }}>
          {taskRunning ? <Tag color="processing">{t("app.syncing")}</Tag> : <Tag>{t("app.idle")}</Tag>}
        </div>
      </Sider>
      <Layout>
        <Header
          style={{
            background: token.colorBgContainer,
            borderBottom: `1px solid ${token.colorSplit}`,
            padding: "0 24px",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            height: 48,
          }}
        >
          <Space
            size={8}
            style={{ cursor: "pointer" }}
            onClick={() => setPage("login")}
            title={t("app.menu.login")}
          >
            {login?.loggedIn && login.avatarUrl && (
              <Avatar size={28} src={login.avatarUrl} />
            )}
            <Typography.Text type="secondary">
              {login?.loggedIn
                ? t("app.loggedInAs", { name: login.nickname ?? "" })
                : t("app.notLoggedIn")}
            </Typography.Text>
          </Space>
          <Space size={8}>
            <Select<string>
              size="small"
              variant="borderless"
              style={{ width: 84 }}
              value={prefLanguage}
              onChange={changeLanguagePref}
              options={[
                { value: "zh-CN", label: "中文" },
                { value: "en", label: "EN" },
              ]}
            />
            <Select<string>
              size="small"
              variant="borderless"
              style={{ width: 96 }}
              value={prefTheme}
              onChange={changeThemePref}
              options={[
                { value: "system", label: t("app.themeSystemShort") },
                { value: "light", label: t("app.themeLightShort") },
                { value: "dark", label: t("app.themeDarkShort") },
              ]}
            />
            <SyncHeaderProgress running={syncRunning} paused={syncPaused} />
            {syncRunning && !syncPaused && (
              <Button size="small" onClick={() => api.pauseSync()}>
                {t("app.pause")}
              </Button>
            )}
            {syncRunning && syncPaused && (
              <Button size="small" type="primary" onClick={() => api.resumeSync()}>
                {t("app.resume")}
              </Button>
            )}
            {syncRunning && (
              <Popconfirm
                title={t("app.cancelConfirm")}
                okText={t("app.cancel")}
                cancelText={t("playlists.cancel")}
                onConfirm={() => api.cancelSync()}
              >
                <Button size="small" danger>
                  {t("app.cancelTask")}
                </Button>
              </Popconfirm>
            )}
          </Space>
        </Header>
        <Content style={{ overflow: "auto", background: token.colorBgLayout }}>
          {updateVersion && (
            <Alert
              banner
              type="info"
              showIcon
              message={t("playlists.updateAvailable", { version: `v${updateVersion}` })}
              action={
                <Button
                  size="small"
                  type="link"
                  onClick={() => {
                    const url = `https://github.com/muxinxy/music-auto-sync/releases/tag/v${updateVersion}`;
                    openUrl(url).catch(() => {});
                  }}
                >
                  {t("playlists.updateGo")}
                </Button>
              }
              closable
              onClose={() => setUpdateVersion(null)}
              style={{ borderRadius: 0 }}
            />
          )}
          {!appReady ? null : page === "login" ? (
            <LoginPage login={login} onLogin={refreshLogin} onLogout={onLogout} />
          ) : page === "playlists" ? (
            <PlaylistsPage login={login} sync={sync} />
          ) : page === "cloud" ? (
            <CloudPage
              login={login}
              onGoSync={() => setPage("sync")}
              onGoSettings={() => setPage("settings")}
            />
          ) : page === "sync" ? (
            <SyncPage />
          ) : page === "quarantine" ? (
            <QuarantinePage />
          ) : page === "about" ? (
            <AboutPage />
          ) : (
            <SettingsPage />
          )}
        </Content>
      </Layout>
    </Layout>
  );
}

/**
 * Header 顶部进度文案：单独订阅高频 progress store，
 * 使每曲目进度更新只重渲染该小组件，不波及 App/侧栏/歌单页。
 */
function SyncHeaderProgress({ running, paused }: { running: boolean; paused: boolean }) {
  const { t } = useTranslation();
  const progress = useSyncExternalStore(syncStore.subscribeProgress, syncStore.getProgress);

  if (!running) return null;
  const progressPhase = progress
    ? t(`phases.${progress.phase}`, { defaultValue: progress.phase })
    : "";
  return (
    <Typography.Text type={paused ? "warning" : "secondary"} style={{ fontSize: 12 }}>
      {paused
        ? t("app.syncPaused")
          : progress
            ? t("app.progressHeader", {
                name: taskDisplayName(progress.playlistName),
                phase: progressPhase,
                current: progress.current,
                total: progress.total,
              })
            : t("app.syncing")}
      {progress?.message && !paused
        ? ` · ${translateProgressMessage(progress.message)}`
        : ""}
    </Typography.Text>
  );
}

function translateProgressMessage(message: UiMessage): string {
  if (message.code === "track" && message.params?.[0]) {
    return message.params[0];
  }
  return translateUi(message);
}