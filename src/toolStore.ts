import type { ToolProgress } from "./types";

/**
 * 工具箱后台任务（NCM 转换 / 重复清理扫描 / 属性修复）的运行状态外部 store。
 * App.tsx 监听 tool://progress 与 tool://state 事件后喂数据，组件按需订阅。
 */
export interface ToolState {
  progress: ToolProgress | null;
  running: boolean;
  /** 任务结束（done）时的本地时间戳，供"刚结束仍在任务卡短暂展示"用。 */
  finishedAt?: number;
}

const state: Record<string, ToolState> = {};
let snapshot: Record<string, ToolState> = {};
const listeners = new Set<() => void>();

function emitChange() {
  // 必须重建快照：useSyncExternalStore 用 Object.is 比较，返回同一引用会判定"无变化"。
  snapshot = { ...state };
  for (const listener of listeners) listener();
}

export const toolStore = {
  onProgress(payload: ToolProgress) {
    const prev = state[payload.kind];
    state[payload.kind] = {
      progress: payload,
      running: !payload.done,
      finishedAt: payload.done ? prev?.finishedAt ?? Date.now() : undefined,
    };
    emitChange();
  },
  onState(payload: { kind: string; running: boolean }) {
    const current = state[payload.kind];
    state[payload.kind] = {
      progress: current?.progress ?? null,
      running: payload.running,
      finishedAt: payload.running ? undefined : current?.finishedAt,
    };
    emitChange();
  },
  subscribe(listener: () => void) {
    listeners.add(listener);
    return () => listeners.delete(listener);
  },
  getSnapshot(): Record<string, ToolState> {
    return snapshot;
  },
  isRunning(kind: string): boolean {
    return state[kind]?.running ?? false;
  },
  progress(kind: string): ToolProgress | null {
    return state[kind]?.progress ?? null;
  },
};
