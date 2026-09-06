import type { CloudTaskRow, SyncProgress } from "./types";

/**
 * 云盘任务的最小订阅存储（与 syncStore 同款模式）。
 * 云盘任务与歌单同步**并行运行**，拥有独立的运行/暂停状态与进度：
 * - running/paused：低频，来自 cloud://state 事件与 get_cloud_control 轮询校准。
 * - progress：高频（cloud://progress），只重渲染订阅它的进度小组件。
 * - rows：本轮任务的对比/上传明细（cloud://row 按 path 实时合并），任务结束后保留，
 *   供「最近结果」或进度上的「任务详情」回看；新一轮开始（cloud://start）时清空。
 */

type Listener = () => void;

let running = false;
let paused = false;
let progress: SyncProgress | null = null;
let rows: CloudTaskRow[] = [];
/** 最近一次云盘任务的 sync_logs 行 id（cloud://start 携带）；
 * 同步日志的任务详情对这一 run 直接复用内存明细（实时且含比对记录）。 */
let currentRunId: number | null = null;

const runningListeners = new Set<Listener>();
const progressListeners = new Set<Listener>();
const rowListeners = new Set<Listener>();
const runIdListeners = new Set<Listener>();

function emit(listeners: Set<Listener>) {
  for (const l of listeners) l();
}

export const cloudStore = {
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
      // 任务已结束但还残留进度也要清掉，避免进度条卡停在旧阶段。
      if (!value && progress !== null) {
        progress = null;
        emit(progressListeners);
      }
      return;
    }
    running = value;
    if (nextPaused !== undefined) paused = nextPaused;
    else if (!value) paused = false;
    if (!running && progress !== null) {
      progress = null;
      emit(progressListeners);
    }
    emit(runningListeners);
  },
  setPaused(value: boolean) {
    if (paused === value) return;
    paused = value;
    emit(runningListeners);
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
    emit(progressListeners);
  },

  // --- 明细 rows ---
  getRows() {
    return rows;
  },
  subscribeRows(listener: Listener) {
    rowListeners.add(listener);
    return () => rowListeners.delete(listener);
  },
  getRunId() {
    return currentRunId;
  },
  subscribeRunId(listener: Listener) {
    runIdListeners.add(listener);
    return () => runIdListeners.delete(listener);
  },
  /** 新一轮云盘任务开始：清空上一轮明细并记录本轮 run id。 */
  start(runId: number) {
    rows = [];
    currentRunId = runId;
    emit(rowListeners);
    emit(runIdListeners);
  },
  /** 按 path 合并一行：已存在则覆盖状态字段，否则追加。 */
  upsert(row: CloudTaskRow) {
    const index = rows.findIndex((r) => r.path === row.path);
    if (index >= 0) {
      const next = rows.slice();
      next[index] = { ...next[index], ...row };
      rows = next;
    } else {
      rows = [...rows, row];
    }
    emit(rowListeners);
  },
};
