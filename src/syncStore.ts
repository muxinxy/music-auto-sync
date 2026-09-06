import type { SyncProgress } from "./types";

/**
 * 同步状态的最小订阅存储：把“高频”的下载进度与“低频”的运行状态拆开，
 * 避免每个 progress 事件都触发整棵 React 树重渲染（App/侧栏/歌单页都无关）。
 * - App / 侧栏只订阅 running/paused（低频）。
 * - 顶部进度文案/进度条单独订阅 progress（高频），更新不波及无关子树。
 */

type Listener = () => void;

let running = false;
let paused = false;
let progress: SyncProgress | null = null;

const runningListeners = new Set<Listener>();
const progressListeners = new Set<Listener>();

function emitRunning() {
  for (const l of runningListeners) l();
}

function emitProgress() {
  for (const l of progressListeners) l();
}

export const syncStore = {
  // --- running/paused（低频） ---
  getRunning() {
    return running;
  },
  getPaused() {
    return paused;
  },
  subscribeRunning(listener: Listener) {
    runningListeners.add(listener);
    return () => runningListeners.delete(listener);
  },
  setRunning(value: boolean, nextPaused?: boolean) {
    if (running === value && (nextPaused === undefined || paused === nextPaused)) {
      // 任务已结束但还残留进度（如失败/取消后重复收到 state:false）也要清掉。
      if (!value && progress !== null) {
        progress = null;
        emitProgress();
      }
      return;
    }
    running = value;
    if (nextPaused !== undefined) paused = nextPaused;
    else if (!value) paused = false;
    // 任务结束（无论成功/失败/取消）时清掉残留进度，避免“当前任务”卡停在旧阶段（0%）。
    if (!running && progress !== null) {
      progress = null;
      emitProgress();
    }
    emitRunning();
  },
  setPaused(value: boolean) {
    if (paused === value) return;
    paused = value;
    emitRunning();
  },

  // --- progress（高频） ---
  getProgress() {
    return progress;
  },
  subscribeProgress(listener: Listener) {
    progressListeners.add(listener);
    return () => progressListeners.delete(listener);
  },
  setProgress(value: SyncProgress | null) {
    progress = value;
    emitProgress();
  },
};
