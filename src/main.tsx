import React, { useEffect, useMemo, useState } from "react";
import ReactDOM from "react-dom/client";
import { ConfigProvider, theme as antdTheme } from "antd";
import zhCN from "antd/locale/zh_CN";
import enUS from "antd/locale/en_US";
import i18n, { normalizeLanguage } from "./i18n";
import { api } from "./api";
import App from "./App";

function resolveTheme(pref: string, systemDark: boolean): "light" | "dark" {
  if (pref === "light") return "light";
  if (pref === "dark") return "dark";
  return systemDark ? "dark" : "light";
}

/** 暗色下的柔和覆盖：背景退到深灰而非纯黑、边框/分割线降对比，减轻“白色刺眼”。 */
const softDarkTokens = {
  colorBgLayout: "#141414",
  colorBgContainer: "#1f1f1f",
  colorBgElevated: "#262626",
  colorBorder: "#3a3a3a",
  colorBorderSecondary: "#333333",
  colorSplit: "#333333",
};

function Root() {
  const [language, setLanguage] = useState<string>(i18n.language);
  const [themePref, setThemePref] = useState<string>("system");
  const [systemDark, setSystemDark] = useState<boolean>(
    () => window.matchMedia?.("(prefers-color-scheme: dark)").matches ?? false
  );

  useEffect(() => {
    // 读取当前主题偏好；随后监听系统主题变化与设置页主题变更事件。
    const loadPref = () =>
      api
        .getConfig()
        .then((cfg) => setThemePref(cfg.theme ?? "system"))
        .catch(() => {});
    loadPref();
    const onThemeChanged = () => loadPref();
    window.addEventListener("theme-changed", onThemeChanged);
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq?.addEventListener?.("change", onChange);
    return () => {
      window.removeEventListener("theme-changed", onThemeChanged);
      mq?.removeEventListener?.("change", onChange);
    };
  }, []);

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

  const mode = resolveTheme(themePref, systemDark);

  useEffect(() => {
    document.documentElement.style.colorScheme = mode;
  }, [mode]);

  const themeConfig = useMemo(
    () => ({
      algorithm: mode === "dark" ? antdTheme.darkAlgorithm : antdTheme.defaultAlgorithm,
      token: {
        colorPrimary: "#c20c0c",
        borderRadius: 6,
        ...(mode === "dark" ? softDarkTokens : {}),
      },
    }),
    [mode]
  );

  return (
    <ConfigProvider
      locale={language === "en" ? enUS : zhCN}
      theme={themeConfig}
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