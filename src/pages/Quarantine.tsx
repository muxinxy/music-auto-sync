import { useCallback, useEffect, useState } from "react";
import { Button, Card, List, Popconfirm, Space, Typography, message as antMessage } from "antd";
import { DeleteOutlined, UndoOutlined } from "@ant-design/icons";
import { api } from "../api";
import type { QuarantineItem } from "../types";

export default function QuarantinePage() {
  const [items, setItems] = useState<QuarantineItem[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await api.listQuarantine());
    } catch (e) {
      antMessage.error(`加载隔离区失败：${e}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  return (
    <div style={{ padding: 24 }}>
      <Card
        title="隔离区"
        extra={<Typography.Text type="secondary">从歌单移除的本地文件会先放在这里，不会直接删除</Typography.Text>}
        styles={{ body: { padding: 0 } }}
      >
        <List
          loading={loading}
          dataSource={items}
          locale={{ emptyText: "隔离区为空" }}
          renderItem={(item) => (
            <List.Item
              style={{ paddingLeft: 24, paddingRight: 24 }}
              actions={[
                <Button
                  key="restore"
                  icon={<UndoOutlined />}
                  onClick={async () => {
                    try {
                      await api.restoreQuarantine(item.id);
                      antMessage.success("已恢复原文件");
                      load();
                    } catch (e) { antMessage.error(`恢复失败：${e}`); }
                  }}
                >
                  恢复
                </Button>,
                <Popconfirm
                  key="delete"
                  title="彻底删除此文件？"
                  description="删除后无法通过本软件恢复。"
                  okText="删除"
                  cancelText="取消"
                  okButtonProps={{ danger: true }}
                  onConfirm={async () => {
                    try {
                      await api.deleteQuarantine(item.id);
                      antMessage.success("文件已删除");
                      load();
                    } catch (e) { antMessage.error(`删除失败：${e}`); }
                  }}
                >
                  <Button danger icon={<DeleteOutlined />}>删除</Button>
                </Popconfirm>,
              ]}
            >
              <List.Item.Meta
                title={item.fileName}
                description={
                  <Space direction="vertical" size={0}>
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      歌单：{item.playlistName} · 隔离时间：{item.quarantinedAt}
                    </Typography.Text>
                    <Typography.Text ellipsis style={{ maxWidth: 600, fontSize: 12 }} type="secondary">
                      原路径：{item.originalPath}
                    </Typography.Text>
                  </Space>
                }
              />
            </List.Item>
          )}
        />
      </Card>
    </div>
  );
}
