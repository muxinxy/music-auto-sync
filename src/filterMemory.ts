/**
 * 页面筛选条件的本地记忆（localStorage）：切换页面后回来保持上次的筛选。
 * 每个页面一个 key，值为可 JSON 序列化的对象；损坏/缺失时返回默认值。
 */
export function loadFilters<T extends object>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<T>;
    return { ...fallback, ...parsed };
  } catch {
    return fallback;
  }
}

export function saveFilters<T extends object>(key: string, value: T): void {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // 存储不可用时静默忽略
  }
}

const MAX_HISTORY = 8;

/** 读取某个搜索框的历史关键词（最近的在前）。 */
export function loadSearchHistory(key: string): string[] {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((v) => typeof v === "string") : [];
  } catch {
    return [];
  }
}

/** 记录一个搜索关键词（去重、最多 8 个）。 */
export function pushSearchHistory(key: string, keyword: string): string[] {
  const word = keyword.trim();
  if (!word) return loadSearchHistory(key);
  const next = [word, ...loadSearchHistory(key).filter((w) => w !== word)].slice(0, MAX_HISTORY);
  try {
    localStorage.setItem(key, JSON.stringify(next));
  } catch {
    // 忽略
  }
  return next;
}

/** 删除单个历史关键词。 */
export function removeSearchHistory(key: string, keyword: string): string[] {
  const next = loadSearchHistory(key).filter((w) => w !== keyword);
  try {
    localStorage.setItem(key, JSON.stringify(next));
  } catch {
    // 忽略
  }
  return next;
}

/** 清空某搜索框的全部历史。 */
export function clearSearchHistory(key: string): void {
  try {
    localStorage.removeItem(key);
  } catch {
    // 忽略
  }
}
