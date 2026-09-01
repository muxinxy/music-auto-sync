import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Button, Card, Result, Space, Spin, Typography } from "antd";
import { QrcodeOutlined, UserOutlined } from "@ant-design/icons";
import { api } from "../api";
import type { LoginStatus } from "../types";

interface Props {
  login: LoginStatus | null;
  onLogin: (verifyAttempt?: number, retryLimit?: number) => Promise<LoginStatus | null>;
  onLogout: () => void;
}

const STATUS_RETRY_LIMIT = 5;
const STATUS_RETRY_DELAY_MS = 1500;

export default function LoginPage({ login, onLogin, onLogout }: Props) {
  const [qrImg, setQrImg] = useState<string | null>(null);
  const [qrState, setQrState] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const keyRef = useRef<string | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const sessionRef = useRef(0);

  const stopPolling = useCallback(() => {
    sessionRef.current += 1;
    keyRef.current = null;
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const verifyLogin = useCallback(async (session: number) => {
    for (let attempt = 0; attempt < STATUS_RETRY_LIMIT; attempt += 1) {
      if (session !== sessionRef.current) return false;
      const status = await onLogin(attempt + 1, STATUS_RETRY_LIMIT);
      if (status?.loggedIn) return true;
      if (attempt < STATUS_RETRY_LIMIT - 1) {
        setQrState(`已授权，正在验证登录状态…（${attempt + 1}/${STATUS_RETRY_LIMIT}）`);
        await new Promise<void>((resolve) => {
          timerRef.current = setTimeout(resolve, STATUS_RETRY_DELAY_MS);
        });
      }
    }
    return false;
  }, [onLogin]);

  const startQr = useCallback(async () => {
    stopPolling();
    const session = sessionRef.current;
    setLoading(true);
    setQrImg(null);
    setQrState("正在获取二维码…");
    try {
      const qr = await api.getLoginQr();
      if (session !== sessionRef.current) return;
      setQrImg(qr.qrImg);
      keyRef.current = qr.key;
      setQrState("请用网易云音乐 App 扫码");

      const poll = async () => {
        if (session !== sessionRef.current || !keyRef.current) return;
        try {
          const result = await api.checkLoginQr(keyRef.current);
          if (session !== sessionRef.current) return;
          setQrState(result.message);
          if (result.state === "success") {
            keyRef.current = null;
            setQrState("已授权，正在验证登录状态…");
            if (await verifyLogin(session)) {
              setQrState("登录成功");
              return;
            }
            setQrState("二维码已授权，但登录状态尚未确认。请稍候点击刷新二维码，或检查 API 地址和代理设置。");
            return;
          }
          if (result.state === "expired") {
            timerRef.current = setTimeout(startQr, 1500);
            return;
          }
        } catch (error) {
          const detail = error instanceof Error ? error.message : String(error);
          setQrState(`二维码状态检查失败：${detail}`);
        }
        if (session === sessionRef.current) {
          timerRef.current = setTimeout(poll, 3000);
        }
      };
      timerRef.current = setTimeout(poll, 2000);
    } catch (error) {
      if (session === sessionRef.current) {
        const detail = error instanceof Error ? error.message : String(error);
        setQrState(`获取二维码失败：${detail}`);
      }
    } finally {
      if (session === sessionRef.current) setLoading(false);
    }
  }, [stopPolling, verifyLogin]);

  useEffect(() => {
    if (!login?.loggedIn) startQr();
    return stopPolling;
  }, [login?.loggedIn, startQr, stopPolling]);

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
          <Button size="small" type="text" loading={loading} onClick={startQr}>
            刷新二维码
          </Button>
          <Button size="small" type="text" onClick={() => api.openLoginLogDirectory()}>
            打开登录日志目录
          </Button>
          <Alert
            type="info"
            showIcon
            message="登录凭据仅保存在本地数据目录中。若验证超时，可在设置中检查 API 地址和 HTTP(S) 代理。"
          />
        </Space>
      </Card>
    </div>
  );
}
