import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button, Card, Result, Space, Spin, Typography } from "antd";
import { QrcodeOutlined, UserOutlined } from "@ant-design/icons";
import { api } from "../api";
import type { LoginStatus } from "../types";

interface Props {
  login: LoginStatus | null;
  onLogin: () => Promise<LoginStatus | null>;
  onLogout: () => void;
}

export default function LoginPage({ login, onLogin, onLogout }: Props) {
  const [qrImg, setQrImg] = useState<string | null>(null);
  const [qrState, setQrState] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const keyRef = useRef<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const stopPolling = () => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  };

  const startQr = useCallback(async () => {
    stopPolling();
    setLoading(true);
    setQrState("正在获取二维码…");
    try {
      const qr = await api.getLoginQr();
      setQrImg(qr.qrImg);
      keyRef.current = qr.key;
      setQrState("请用网易云音乐 App 扫码");

      const poll = async () => {
        if (!keyRef.current) return;
        try {
          const r = await api.checkLoginQr(keyRef.current);
          setQrState(r.message);
          if (r.state === "success") {
            keyRef.current = null;
            await onLogin();
            setQrState("登录成功");
            return;
          }
          if (r.state === "expired") {
            // 自动刷新二维码
            setTimeout(startQr, 1500);
            return;
          }
        } catch {
          setQrState("网络异常，重试中…");
        }
        timerRef.current = setTimeout(poll, 2000);
      };
      timerRef.current = setTimeout(poll, 2000);
    } catch (e) {
      setQrState(`获取二维码失败：${e}`);
    } finally {
      setLoading(false);
    }
  }, [onLogin]);

  useEffect(() => {
    if (!login?.loggedIn) startQr();
    return stopPolling;
  }, [login?.loggedIn, startQr]);

  if (login?.loggedIn) {
    return (
      <div style={{ padding: 24 }}>
        <Card style={{ maxWidth: 560 }}>
          <Result
            icon={<UserOutlined style={{ color: "#c20c0c" }} />}
            title={`已登录：${login.nickname ?? ""}`}
            subTitle="登录态过期时会在此页面提示重新扫码"
            extra={
              <Button danger onClick={async () => { await api.logout(); onLogout(); }}>
                退出登录
              </Button>
            }
          />
        </Card>
      </div>
    );
  }

  return (
    <div style={{ padding: 24 }}>
      <Card style={{ maxWidth: 560 }}>
        <Space direction="vertical" align="center" style={{ width: "100%" }} size="large">
          <Typography.Title level={4} style={{ marginBottom: 0 }}>
            <QrcodeOutlined /> 网易云音乐扫码登录
          </Typography.Title>
          <div
            style={{
              width: 220,
              height: 220,
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              background: "#fafafa",
              borderRadius: 8,
            }}
          >
            {qrImg ? (
              <img src={qrImg} alt="二维码" style={{ width: 200, height: 200 }} />
            ) : (
              <Spin />
            )}
          </div>
          <Typography.Text type={qrState.includes("成功") ? "success" : "secondary"}>
            {qrState}
          </Typography.Text>
          <Button size="small" type="text" onClick={startQr}>
            刷新二维码
          </Button>
          <Alert
            type="info"
            showIcon
            message="登录仅用于获取你的歌单和下载地址，凭据保存在本地数据目录中。"
          />
        </Space>
      </Card>
    </div>
  );
}
