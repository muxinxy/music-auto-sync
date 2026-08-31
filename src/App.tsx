import { useCallback, useEffect, useState } from "react";
import { Layout, Menu, Tag, Typography, App as AntApp } from "antd";
import {
  CloudSyncOutlined,
  HistoryOutlined,
  LoginOutlined,
  ReloadOutlined,
  DeleteOutlined,
  SettingOutlined,
} from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import type { LoginStatus, SyncProgress } from "./types";
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
  const [page, setPage] = useState<PageKey>("playlists");
  const [login, setLogin] = useState<LoginStatus | null>(null);
  const [sync, setSync] = useState<SyncEventState>({ running: false });
  const [appReady, setAppReady] = useState(false);

  const refreshLogin = useCallback(async () => {
    try {
      const s = await api.getLoginStatus();
      setLogin(s);
      return s;
    } catch {
      return null;
    }
  }, []);

  useEffect(() => {
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
  }, [refreshLogin]);

  const onLogout = () => {
    setLogin(null);
    message.info("已退出登录");
    setPage("login");
  };

  const items = [
    { key: "login", icon: <LoginOutlined />, label: "账号登录" },
    { key: "playlists", icon: <CloudSyncOutlined />, label: "歌单同步" },
    { key: "sync", icon: <HistoryOutlined />, label: "同步任务" },
    { key: "quarantine", icon: <DeleteOutlined />, label: "隔离区" },
    { key: "settings", icon: <SettingOutlined />, label: "设置" },
  ];

  return (
    <Layout style={{ height: "100vh" }}>
      <Sider theme="dark" width={200}>
        <div style={{ padding: "20px 16px", color: "#fff" }}>
          <ReloadOutlined style={{ color: "#c20c0c", marginRight: 8 }} />
          <span style={{ fontWeight: 600 }}>音乐同步</span>
        </div>
        <Menu
          theme="dark"
          mode="inline"
          selectedKeys={[page]}
          items={items}
          onClick={(e) => setPage(e.key as PageKey)}
        />
        <div style={{ position: "absolute", bottom: 12, left: 16, color: "#888", fontSize: 12 }}>
          {sync.running ? (
            <Tag color="processing">同步中…</Tag>
          ) : (
            <Tag>空闲</Tag>
          )}
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
              ? `已登录：${login.nickname ?? ""}`
              : "未登录 —— 扫码登录后才能同步歌单"}
          </Typography.Text>
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {sync.running && sync.progress
              ? `${sync.progress.playlistName} · ${sync.progress.phase} ${sync.progress.current}/${sync.progress.total}`
              : ""}
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
