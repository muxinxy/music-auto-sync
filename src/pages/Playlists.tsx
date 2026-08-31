import { useCallback, useEffect, useState } from "react";
import {
  Alert,
  Avatar,
  Button,
  Card,
  Input,
  List,
  Space,
  Switch,
  Tag,
  Typography,
  message as antMessage,
} from "antd";
import { SyncOutlined } from "@ant-design/icons";
import { api } from "../api";
import type { LoginStatus, PlaylistInfo } from "../types";
import type { SyncEventState } from "../App";

interface Props {
  login: LoginStatus | null;
  sync: SyncEventState;
}

export default function PlaylistsPage({ login, sync }: Props) {
  const [playlists, setPlaylists] = useState<PlaylistInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [filter, setFilter] = useState("");

  const load = useCallback(async () => {
    if (!login?.loggedIn) return;
    setLoading(true);
    try {
      setPlaylists(await api.listPlaylists());
    } catch (e) {
      antMessage.error(`加载歌单失败：${e}`);
    } finally {
      setLoading(false);
    }
  }, [login?.loggedIn]);

  useEffect(() => {
    load();
  }, [load]);

  if (!login?.loggedIn) {
    return (
      <div style={{ padding: 24 }}>
        <Alert message="请先在「账号登录」页扫码登录，再管理歌单同步。" type="warning" showIcon />
      </div>
    );
  }

  const shown = playlists.filter((p) =>
    p.name.toLowerCase().includes(filter.toLowerCase())
  );

  return (
    <div style={{ padding: 24 }}>
      <Card style={{ marginBottom: 16 }}>
        <Space style={{ width: "100%", justifyContent: "space-between" }}>
          <Space>
            <Input.Search
              placeholder="搜索歌单"
              allowClear
              style={{ width: 240 }}
              onSearch={setFilter}
              onChange={(e) => !e.target.value && setFilter("")}
            />
            <Typography.Text type="secondary">
              共 {playlists.length} 个歌单，开启开关即可纳入自动同步
            </Typography.Text>
          </Space>
          <Space>
            <Button icon={<SyncOutlined />} onClick={load} disabled={sync.running}>
              刷新
            </Button>
            <Button
              type="primary"
              icon={<SyncOutlined spin={sync.running} />}
              loading={sync.running}
              onClick={async () => {
                try {
                  await api.syncAll();
                  antMessage.success("全部歌单同步完成");
                  load();
                } catch (e) {
                  antMessage.error(`同步失败：${e}`);
                }
              }}
            >
              立即同步全部
            </Button>
          </Space>
        </Space>
      </Card>

      <Card styles={{ body: { padding: 0 } }}>
        <List
          loading={loading}
          dataSource={shown}
          renderItem={(p) => (
            <List.Item
              actions={[
                <Switch
                  key="sw"
                  checked={p.enabled}
                  checkedChildren="同步"
                  unCheckedChildren="关闭"
                  onChange={async (v) => {
                    await api.setPlaylistEnabled(p.id, v);
                    load();
                  }}
                />,
                <Button
                  key="go"
                  size="small"
                  disabled={sync.running}
                  onClick={async () => {
                    try {
                      await api.syncPlaylist(p.id);
                      antMessage.success(`「${p.name}」同步完成`);
                      load();
                    } catch (e) {
                      antMessage.error(`同步失败：${e}`);
                    }
                  }}
                >
                  立即同步
                </Button>,
              ]}
            >
              <List.Item.Meta
                avatar={
                  <Avatar shape="square" size={48} src={p.coverImgUrl} />
                }
                title={
                  <Space>
                    <span>{p.name}</span>
                    {p.subscribed && <Tag>收藏</Tag>}
                  </Space>
                }
                description={
                  <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                    {p.trackCount} 首
                    {p.lastSync ? ` · 上次同步 ${p.lastSync}` : " · 从未同步"}
                    {p.lastResult ? ` · ${p.lastResult}` : ""}
                  </Typography.Text>
                }
              />
            </List.Item>
          )}
        />
      </Card>
    </div>
  );
}
