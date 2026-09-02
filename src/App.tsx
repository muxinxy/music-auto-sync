import { useCallback, useEffect, useState } from "react";
import { Layout, Menu, Tag, Typography, App as AntApp } from "antd";
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
  progress?: SyncProgress;
}

export default function App() {
  const { message } = AntApp.useApp();
  const { t } = useTranslation();
  const [page, setPage] = useState<PageKey>("playlists");
  const [login, setLogin] = useState<LoginStatus | null>(null);
  const [sync, setSync] = useState<SyncEventState>({ running: false });
  const [appReady, setAppReady] = useState(false);

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
    refreshLogin().finally(() => setAppReady(true));

    const unlistenProgress = listen<SyncProgress>("sync://progress", (e) => {
      setSync((s) => ({ ...s, progress: e.payload }));
    });
    const unlistenState = listen<boolean>("sync://state", (e) => {
      setSync((s) => ({ ...s, running: e.payload }));
      if (e.payload === false) refreshLogin();
    });

    return () => {
      unlistenProgress.then((f) => f());
      unlistenState.then((f) => f());
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
          <Typography.Text type="secondary">
            {login?.loggedIn
              ? t("app.loggedInAs", { name: login.nickname ?? "" })
              : t("app.notLoggedIn")}
          </Typography.Text>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {sync.running && sync.progress
              ? t("app.progressHeader", {
                  name: sync.progress.playlistName,
                  phase: progressPhase,
                  current: sync.progress.current,
                  total: sync.progress.total,
                })
              : ""}
            {sync.progress?.message ? ` · ${translateProgressMessage(sync.progress.message)}` : ""}
          </Typography.Text>
        </Header>
        <Content style={{ overflow: "auto", background: "#f5f5f5" }}>
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