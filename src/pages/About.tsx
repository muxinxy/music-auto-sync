import { useEffect, useState } from "react";
import { Card, Space, Typography } from "antd";
import {
  GithubOutlined,
  GlobalOutlined,
  ReadOutlined,
} from "@ant-design/icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import type { AppInfo } from "../types";

const REPO_URL = "https://github.com/muxinxy/music-auto-sync";
const API_REPO_URL = "https://github.com/neteasecloudmusicapienhanced/api-enhanced";

/** 关于页：版本/技术栈/数据说明/外部链接。 */
export default function AboutPage() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    api.getAppInfo().then(setInfo).catch(() => {});
  }, []);

  const open = (url: string) => {
    openUrl(url).catch(() => {});
  };

  return (
    <div style={{ maxWidth: 640, padding: 8 }}>
      <Card size="small" style={{ marginBottom: 16 }}>
        <Space direction="vertical" size={4}>
          <Typography.Title level={4} style={{ marginBottom: 0 }}>
            {t("app.brand")}
          </Typography.Title>
          <Typography.Text type="secondary">
            {t("about.tagline")}
          </Typography.Text>
          <Typography.Text>
            {t("about.version", { version: info?.version ?? "-" })}
          </Typography.Text>
        </Space>
      </Card>

      <Card size="small" title={t("about.cardTech")} style={{ marginBottom: 16 }}>
        <Typography.Paragraph style={{ marginBottom: 8 }}>
          {t("about.techStack")}
        </Typography.Paragraph>
        <Typography.Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 0 }}>
          {t("about.techDetail")}
        </Typography.Paragraph>
      </Card>

      <Card size="small" title={t("about.cardPrivacy")} style={{ marginBottom: 16 }}>
        <Typography.Paragraph style={{ marginBottom: 4 }}>
          {t("about.privacyLocal")}
        </Typography.Paragraph>
        <Typography.Paragraph type="secondary" style={{ fontSize: 13, marginBottom: 0 }}>
          {t("about.privacyDetail")}
        </Typography.Paragraph>
      </Card>

      <Card size="small" title={t("about.cardLinks")}>
        <Space direction="vertical" size={8} style={{ width: "100%" }}>
          <Typography.Link onClick={() => open(REPO_URL)}>
            <GithubOutlined /> {t("about.repo")}
          </Typography.Link>
          <Typography.Link onClick={() => open(`${REPO_URL}/releases`)}>
            <ReadOutlined /> {t("about.releases")}
          </Typography.Link>
          <Typography.Link onClick={() => open(API_REPO_URL)}>
            <GlobalOutlined /> {t("about.apiRepo")}
          </Typography.Link>
        </Space>
      </Card>
    </div>
  );
}
