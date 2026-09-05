import { useCallback, useEffect, useState } from "react";
import { Alert, Avatar, Button, Layout, Menu, Popconfirm, Space, Tag, Typography, App as AntApp } from "antd";
import {
  CloudSyncOutlined,
  DeleteOutlined,
  HistoryOutlined,
  LoginOutlined,
  ReloadOutlined,
  SettingOutlined,
} from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import i18n, { normalizeLanguage } from "./i18n";
import { api } from "./api";
import type { LoginStatus, SyncProgress, UiMessage } from "./types";
import { translateUi } from "./errors";
import LoginPage from "./pages/Login";
import PlaylistsPage from "./pages/Playlists";
import SyncPage from "./pages/Sync";
import QuarantinePage from "./pages/Quarantine";
import SettingsPage from "./pages/Settings";

const { Sider, Content, Header } = Layout;

export type PageKey = "login" | "playlists" | "sync" | "quarantine" | "settings";

export interface SyncEventState {
  running: boolean;
  paused?: boolean;
  progress?: SyncProgress;
}

export default function App() {
  const { message } = AntApp.useApp();
  const { t } = useTranslation();
  const [page, setPage] = useState<PageKey>("login");
  const [login, setLogin] = useState<LoginStatus | null>(null);
  const [sync, setSync] = useState<SyncEventState>({ running: false });
  const [appReady, setAppReady] = useState(false);
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);

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
    // 启动始终停留在“账号登录”页（登录后该页显示账号信息与统计）；
    // 已登录也由用户自行点击左侧菜单进入歌单页。
    refreshLogin().finally(() => setAppReady(true));

    api.checkForUpdate().then((version) => {
      if (version) setUpdateVersion(version);
    }).catch(() => {});

    const unlistenProgress = listen<SyncProgress>("sync://progress", (e) => {
      setSync((s) => ({ ...s, progress: e.payload }));
    });
    const unlistenState = listen<boolean>("sync://state", (e) => {
      setSync((s) => ({ ...s, running: e.payload, paused: e.payload ? s.paused : false }));
      if (e.payload === false) refreshLogin();
    });
    // 轮询同步控制状态（暂停/继续），保持 UI 与后端一致。
    const poll = setInterval(async () => {
      try {
        const ctrl = await api.getSyncControl();
        setSync((s) => ({ ...s, running: ctrl.running, paused: ctrl.running ? ctrl.paused : false }));
      } catch {
        // 忽略轮询失败
      }
    }, 1000);

    return () => {
      unlistenProgress.then((f) => f());
      unlistenState.then((f) => f());
      clearInterval(poll);
    };
  }, [applyLanguage, refreshLogin]);

  const onLogout = () => {
    setLogin(null);
    message.info(t("app.loggedOut"));
    setPage("login");
  };

  const progressPhase = sync.progress
    ? t(`phases.${sync.progress.phase}`, { defaultValue: sync.progress.phase })
    : "";

  const items = [
    { key: "login", icon: <LoginOutlined />, label: t("app.menu.login") },
    { key: "playlists", icon: <CloudSyncOutlined />, label: t("app.menu.playlists") },
    { key: "sync", icon: <HistoryOutlined />, label: t("app.menu.sync") },
    { key: "quarantine", icon: <DeleteOutlined />, label: t("app.menu.quarantine") },
    { key: "settings", icon: <SettingOutlined />, label: t("app.menu.settings") },
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
          {sync.running ? <Tag color="processing">{t("app.syncing")}</Tag> : <Tag>{t("app.idle")}</Tag>}
        </div>
      </Sider>
      <Layout>
        <Header
          style={{
            background: "#fff",
            borderBottom: "1px solid #f0f0f0",
            padding: "0 24px",
            display: "flex",
            alignItems: "center",
            justifyContent: "space-between",
            height: 48,
          }}
        >
          <Space size={8}>
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
            {sync.running && (
              <Typography.Text type={sync.paused ? "warning" : "secondary"} style={{ fontSize: 12 }}>
                {sync.paused
                  ? t("app.syncPaused")
                  : sync.progress
                    ? t("app.progressHeader", {
                        name: sync.progress.playlistName,
                        phase: progressPhase,
                        current: sync.progress.current,
                        total: sync.progress.total,
                      })
                    : t("app.syncing")}
                {sync.progress?.message && !sync.paused
                  ? ` · ${translateProgressMessage(sync.progress.message)}`
                  : ""}
              </Typography.Text>
            )}
            {sync.running && !sync.paused && (
              <Button size="small" onClick={() => api.pauseSync()}>
                {t("app.pause")}
              </Button>
            )}
            {sync.running && sync.paused && (
              <Button size="small" type="primary" onClick={() => api.resumeSync()}>
                {t("app.resume")}
              </Button>
            )}
            {sync.running && (
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
        <Content style={{ overflow: "auto", background: "#f5f5f5" }}>
          {updateVersion && (
            <Alert
              banner
              type="info"
              showIcon
              message={t("playlists.updateAvailable", { version: updateVersion })}
              action={
                <Button
                  size="small"
                  type="link"
                  onClick={() => {
                    const url = `https://github.com/muxinxy/music-auto-sync/releases/tag/v${updateVersion}`;
                    window.open(url, "_blank");
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
          ) : page === "sync" ? (
            <SyncPage />
          ) : page === "quarantine" ? (
            <QuarantinePage />
          ) : (
            <SettingsPage />
          )}
        </Content>
      </Layout>
    </Layout>
  );
}

function translateProgressMessage(message: UiMessage): string {
  if (message.code === "track" && message.params?.[0]) {
    return message.params[0];
  }
  return translateUi(message);
}