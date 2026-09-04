import i18n from "./i18n";
import type { UiMessage } from "./types";

/** 把 Tauri 命令返回的错误转换为 UiMessage。后端错误可能是 JSON 字符串化的 UiMessage。 */
export function uiMessage(value: unknown): UiMessage {
  if (typeof value === "string") {
    const trimmed = value.trim();
    if (trimmed.startsWith("{")) {
      try {
        const parsed = JSON.parse(trimmed) as { code?: unknown; params?: unknown };
        if (typeof parsed.code === "string") {
          const params = Array.isArray(parsed.params) ? parsed.params.map(String) : [];
          return { code: parsed.code, params };
        }
      } catch {
        // 不是 JSON，按原文处理
      }
    }
    return { code: "unknown", params: [trimmed] };
  }
  if (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { code?: unknown }).code === "string"
  ) {
    const candidate = value as { code: string; params?: unknown };
    const params = Array.isArray(candidate.params)
      ? candidate.params.map(String)
      : [];
    return { code: candidate.code, params };
  }
  return { code: "unknown", params: [String(value)] };
}

/** 将 UiMessage（含错误码/参数）翻译为当前语言的可读文本。 */
export function translateUi(message: UiMessage): string {
  const key = `errors.${message.code}`;
  const values: Record<string, string> = {};
  (message.params ?? []).forEach((param, index) => {
    values[String(index)] = param;
  });
  return i18n.t(key, { ...values, defaultValue: message.code });
}

export function formatError(error: unknown): string {
  return translateUi(uiMessage(error));
}