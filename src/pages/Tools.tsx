import { useEffect, useState } from "react";
import { Button, Card, Col, Row, Tag, Typography } from "antd";
import { ClearOutlined, FileDoneOutlined, ToolOutlined } from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import i18n from "../i18n";
import { api } from "../api";
import CleanupToolModal from "./CleanupToolModal";
import NcmToolModal from "./NcmToolModal";
import RepairToolModal from "./RepairToolModal";

/** 工具箱页面：集中放置所有独立工具。 */
export default function ToolsPage() {
  const { t } = useTranslation();
  const [ncmOpen, setNcmOpen] = useState(false);
  const [cleanupOpen, setCleanupOpen] = useState(false);
  const [repairOpen, setRepairOpen] = useState(false);
  const [repairRunning, setRepairRunning] = useState(false);

  // 修复任务后台运行：在本页轮询状态，卡片上给出“进行中”提示。
  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | null = null;
    const check = async () => {
      try {
        const control = await api.getToolControl("repair");
        if (cancelled) return;
        setRepairRunning(control.running);
        if (timer && !control.running) {
          clearInterval(timer);
          timer = null;
        }
      } catch {
        // 忽略轮询失败
      }
    };
    check();
    timer = setInterval(check, 1500);
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
    };
  }, [repairOpen]);

  const tools = [
    {
      key: "ncm",
      icon: <ToolOutlined />,
      title: t("tools.ncmCardTitle"),
      desc: t("tools.ncmCardDesc"),
      button: t("tools.ncmOpen"),
      onOpen: () => setNcmOpen(true),
    },
    {
      key: "cleanup",
      icon: <ClearOutlined />,
      title: t("tools.cleanupCardTitle"),
      desc: t("tools.cleanupCardDesc"),
      button: t("tools.cleanupOpen"),
      onOpen: () => setCleanupOpen(true),
    },
    {
      key: "repair",
      icon: <FileDoneOutlined />,
      title: t("tools.repairCardTitle"),
      desc: t("tools.repairCardDesc"),
      button: t("tools.repairOpen"),
      onOpen: () => setRepairOpen(true),
    },
  ];

  return (
    <div style={{ padding: 24 }}>
      <Row gutter={[16, 16]}>
        {tools.map((tool) => (
          <Col key={tool.key} xs={24} md={8}>
            <Card
              hoverable
              style={{ height: "100%" }}
              onClick={tool.onOpen}
              styles={{ body: { display: "flex", flexDirection: "column", gap: 8, height: "100%" } }}
            >
              <Typography.Text strong style={{ fontSize: 16 }}>
                {tool.icon} {tool.title}
                {tool.key === "repair" && repairRunning && (
                  <Tag color="processing" style={{ marginLeft: 8 }}>
                    {i18n.t("tools.repairRunningTag")}
                  </Tag>
                )}
              </Typography.Text>
              <Typography.Paragraph type="secondary" style={{ fontSize: 12, marginBottom: 0 }}>
                {tool.desc}
              </Typography.Paragraph>
              <div>
                <Button type="primary" ghost onClick={tool.onOpen}>
                  {tool.button}
                </Button>
              </div>
            </Card>
          </Col>
        ))}
      </Row>
      <NcmToolModal open={ncmOpen} onClose={() => setNcmOpen(false)} />
      <CleanupToolModal open={cleanupOpen} onClose={() => setCleanupOpen(false)} />
      <RepairToolModal open={repairOpen} onClose={() => setRepairOpen(false)} />
    </div>
  );
}
