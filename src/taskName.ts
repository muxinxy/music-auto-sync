import i18n from "./i18n";

/**
 * 后端任务名的哨兵值 → 当前语言文案；普通歌单名原样返回。
 * 云盘上传等非歌单任务在进度/报告/日志里以哨兵名（如 "cloud"）出现，
 * 后端不感知语言，由前端翻译。
 */
export function taskDisplayName(name: string | null | undefined): string {
  if (!name) return "";
  if (name === "cloud") return i18n.t("cloud.taskName");
  if (name === "cloud_download") return i18n.t("cloud.taskNameDownload");
  return name;
}
