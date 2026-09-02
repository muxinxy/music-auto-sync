import { useCallback, useEffect, useState } from "react";
import { Button, Card, List, Popconfirm, Space, Typography, message as antMessage } from "antd";
import { DeleteOutlined, UndoOutlined } from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { formatError } from "../errors";
import type { QuarantineItem } from "../types";

export default function QuarantinePage() {
  const { t } = useTranslation();
  const [items, setItems] = useState<QuarantineItem[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setItems(await api.listQuarantine());
    } catch (e) {
      antMessage.error(t("quarantine.loadFailed", { detail: formatError(e) }));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div style={{ padding: 24 }}>
      <Card
        title={t("quarantine.title")}
        extra={<Typography.Text type="secondary">{t("quarantine.extra")}</Typography.Text>}
        styles={{ body: { padding: 0 } }}
      >
        <List
          loading={loading}
          dataSource={items}
          locale={{ emptyText: t("quarantine.empty") }}
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
                      antMessage.success(t("quarantine.restored"));
                      load();
                    } catch (e) {
                      antMessage.error(t("quarantine.restoreFailed", { detail: formatError(e) }));
                    }
                  }}
                >
                  {t("quarantine.restore")}
                </Button>,
                <Popconfirm
                  key="delete"
                  title={t("quarantine.confirmTitle")}
                  description={t("quarantine.confirmDesc")}
                  okText={t("quarantine.confirmOk")}
                  cancelText={t("quarantine.confirmCancel")}
                  okButtonProps={{ danger: true }}
                  onConfirm={async () => {
                    try {
                      await api.deleteQuarantine(item.id);
                      antMessage.success(t("quarantine.deleted"));
                      load();
                    } catch (e) {
                      antMessage.error(t("quarantine.deleteFailed", { detail: formatError(e) }));
                    }
                  }}
                >
                  <Button danger icon={<DeleteOutlined />}>
                    {t("quarantine.delete")}
                  </Button>
                </Popconfirm>,
              ]}
            >
              <List.Item.Meta
                title={item.fileName}
                description={
                  <Space direction="vertical" size={0}>
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      {t("quarantine.meta", { playlist: item.playlistName, time: item.quarantinedAt })}
                    </Typography.Text>
                    <Typography.Text ellipsis style={{ maxWidth: 600, fontSize: 12 }} type="secondary">
                      {t("quarantine.originalPath", { path: item.originalPath })}
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