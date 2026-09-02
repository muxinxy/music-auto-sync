import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { ConfigProvider } from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import i18n, { normalizeLanguage } from "./i18n";
import App from "./App";

function Root() {
  const [language, setLanguage] = useState<string>(i18n.language);

  useEffect(() => {
    const onChange = (lng: string) => {
      setLanguage(lng);
      document.documentElement.lang = lng === "en" ? "en" : "zh-CN";
      document.title = lng === "en" ? "Music Auto Sync" : "音乐同步";
    };
    i18n.on("languageChanged", onChange);
    onChange(normalizeLanguage(language));
    return () => {
      i18n.off("languageChanged", onChange);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <ConfigProvider
      locale={language === "en" ? enUS : zhCN}
      theme={{
        token: {
          colorPrimary: "#c20c0c",
          borderRadius: 6,
        },
      }}
    >
      <App />
    </ConfigProvider>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>
);