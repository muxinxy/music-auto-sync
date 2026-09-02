import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import zhCN from "./locales/zh-CN.json";
import en from "./locales/en.json";

export const SUPPORTED_LANGUAGES = ["zh-CN", "en"] as const;
export type Language = (typeof SUPPORTED_LANGUAGES)[number];

i18n.use(initReactI18next).init({
  resources: {
    "zh-CN": { translation: zhCN },
    en: { translation: en },
  },
  lng: "zh-CN",
  fallbackLng: "zh-CN",
  interpolation: { escapeValue: false },
});

export function normalizeLanguage(value: string | null | undefined): Language {
  if (value?.startsWith("en")) return "en";
  return "zh-CN";
}

export default i18n;