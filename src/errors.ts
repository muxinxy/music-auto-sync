import i18n from "./i18n";
import type { UiMessage } from "./types";

export function uiMessage(value: unknown): UiMessage {
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