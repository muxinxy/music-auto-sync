import { useCallback, useEffect, useRef, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Input,
  Result,
  Space,
  Spin,
  Tabs,
  Typography,
  message as antMessage,
} from "antd";
import { QrcodeOutlined, UserOutlined } from "@ant-design/icons";
import { useTranslation } from "react-i18next";
import { api } from "../api";
import { formatError } from "../errors";
import type { LoginStatus } from "../types";

interface Props {
  login: LoginStatus | null;
  onLogin: (verifyAttempt?: number, retryLimit?: number) => Promise<LoginStatus | null>;
  onLogout: () => void;
}

const STATUS_RETRY_LIMIT = 5;
const STATUS_RETRY_DELAY_MS = 1500;

export default function LoginPage({ login, onLogin, onLogout }: Props) {
  const { t } = useTranslation();
  const [qrImg, setQrImg] = useState<string | null>(null);
  const [qrState, setQrState] = useState<string>("");
  const [loginSucceeded, setLoginSucceeded] = useState(false);
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

  const verifyLogin = useCallback(
    async (session: number) => {
      for (let attempt = 0; attempt < STATUS_RETRY_LIMIT; attempt += 1) {
        if (session !== sessionRef.current) return false;
        const status = await onLogin(attempt + 1, STATUS_RETRY_LIMIT);
        if (status?.loggedIn) return true;
        if (attempt < STATUS_RETRY_LIMIT - 1) {
          setQrState(t("login.verifyingCount", { count: attempt + 1, limit: STATUS_RETRY_LIMIT }));
          await new Promise<void>((resolve) => {
            timerRef.current = setTimeout(resolve, STATUS_RETRY_DELAY_MS);
          });
        }
      }
      return false;
    },
    [onLogin, t]
  );

  const startQr = useCallback(async () => {
    stopPolling();
    const session = sessionRef.current;
    setLoading(true);
    setQrImg(null);
    setLoginSucceeded(false);
    setQrState(t("login.qrLoading"));
    try {
      const qr = await api.getLoginQr();
      if (session !== sessionRef.current) return;
      setQrImg(qr.qrImg);
      keyRef.current = qr.key;
      setQrState(t("login.qrScan"));

      const poll = async () => {
        if (session !== sessionRef.current || !keyRef.current) return;
        try {
          const result = await api.checkLoginQr(keyRef.current);
          if (session !== sessionRef.current) return;
          if (result.state === "success") {
            keyRef.current = null;
            setQrState(t("login.verifying"));
            if (await verifyLogin(session)) {
              setLoginSucceeded(true);
              setQrState(t("login.success"));
              return;
            }
            setQrState(t("login.notConfirmed"));
            return;
          }
          setQrState(
            result.state === "expired"
              ? t("login.qrLoading")
              : result.state === "scanned"
                ? t("login.verifying")
                : t("login.qrScan")
          );
          if (result.state === "expired") {
            timerRef.current = setTimeout(startQr, 1500);
            return;
          }
        } catch (error) {
          setQrState(t("login.checkFailed", { detail: formatError(error) }));
        }
        if (session === sessionRef.current) {
          timerRef.current = setTimeout(poll, 3000);
        }
      };
      timerRef.current = setTimeout(poll, 2000);
    } catch (error) {
      if (session === sessionRef.current) {
        setQrState(t("login.getFailed", { detail: formatError(error) }));
      }
    } finally {
      if (session === sessionRef.current) setLoading(false);
    }
  }, [stopPolling, verifyLogin, t]);

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
            title={t("login.loggedInAs", { name: login.nickname ?? "" })}
            subTitle={t("login.subtitle")}
            extra={
              <Button danger onClick={async () => { await api.logout(); onLogout(); }}>
                {t("login.logout")}
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
        <Tabs
          centered
          items={[
            {
              key: "qr",
              label: (
                <span>
                  <QrcodeOutlined /> {t("login.qrTab")}
                </span>
              ),
              children: (
                <Space direction="vertical" align="center" style={{ width: "100%" }} size="large">
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
                      <img src={qrImg} alt={t("login.altQr")} style={{ width: 200, height: 200 }} />
                    ) : (
                      <Spin />
                    )}
                  </div>
                  <Typography.Text type={loginSucceeded ? "success" : "secondary"}>
                    {qrState}
                  </Typography.Text>
                  <Space>
                    <Button size="small" type="text" loading={loading} onClick={startQr}>
                      {t("login.refresh")}
                    </Button>
                    <Button size="small" type="text" onClick={() => api.openLoginLogDirectory()}>
                      {t("login.openLogDir")}
                    </Button>
                  </Space>
                  <Alert type="info" showIcon message={t("login.hint")} />
                </Space>
              ),
            },
            {
              key: "sms",
              label: (
                <span>
                  <UserOutlined /> {t("login.smsTab")}
                </span>
              ),
              children: <SmsLoginForm onLogin={onLogin} />,
            },
          ]}
        />
      </Card>
    </div>
  );
}

function SmsLoginForm({ onLogin }: { onLogin: Props["onLogin"] }) {
  const { t } = useTranslation();
  const [phone, setPhone] = useState("");
  const [code, setCode] = useState("");
  const [countdown, setCountdown] = useState(0);
  const [sending, setSending] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const timerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => () => {
    if (timerRef.current) clearInterval(timerRef.current);
  }, []);

  const startCountdown = () => {
    setCountdown(60);
    if (timerRef.current) clearInterval(timerRef.current);
    timerRef.current = setInterval(() => {
      setCountdown((c) => {
        if (c <= 1) {
          if (timerRef.current) clearInterval(timerRef.current);
          return 0;
        }
        return c - 1;
      });
    }, 1000);
  };

  const sendCode = async () => {
    if (!phone.trim()) {
      antMessage.error(t("login.smsPhoneEmpty"));
      return;
    }
    setSending(true);
    try {
      await api.sendLoginCaptcha(phone.trim());
      antMessage.success(t("errors.smsCodeSent"));
      startCountdown();
    } catch (e) {
      antMessage.error(t("errors.smsCodeFailed", { detail: formatError(e) }));
    } finally {
      setSending(false);
    }
  };

  const submit = async () => {
    if (!phone.trim()) {
      antMessage.error(t("login.smsPhoneEmpty"));
      return;
    }
    if (!code.trim()) {
      antMessage.error(t("login.smsCodeEmpty"));
      return;
    }
    setSubmitting(true);
    try {
      await api.loginWithCaptcha(phone.trim(), code.trim());
      antMessage.success(t("login.success"));
      const status = await onLogin();
      if (!status?.loggedIn) {
        antMessage.info(t("login.notConfirmed"));
      }
    } catch (e) {
      antMessage.error(formatError(e));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Space direction="vertical" style={{ width: "100%" }} size="middle">
      <Typography.Paragraph type="secondary" style={{ fontSize: 12 }}>
        {t("login.smsHint")}
      </Typography.Paragraph>
      <Input
        placeholder={t("login.phonePlaceholder")}
        value={phone}
        maxLength={20}
        onChange={(e) => setPhone(e.target.value.replace(/\D/g, ""))}
      />
      <Space.Compact style={{ width: "100%" }}>
        <Input
          placeholder={t("login.codePlaceholder")}
          value={code}
          maxLength={6}
          onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
        />
        <Button onClick={sendCode} loading={sending} disabled={countdown > 0}>
          {countdown > 0 ? t("login.sendCodeCount", { count: countdown }) : t("login.sendCode")}
        </Button>
      </Space.Compact>
      <Button type="primary" block loading={submitting} onClick={submit}>
        {t("login.smsLogin")}
      </Button>
    </Space>
  );
}