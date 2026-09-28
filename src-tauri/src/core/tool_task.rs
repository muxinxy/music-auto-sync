//! 工具箱后台任务的状态框架：NCM 转换、重复清理扫描、属性修复共用。
//!
//! 每类任务一个 `ToolTaskState`（AppState 持有）：运行/取消标志 + 进度快照 +
//! 逐文件结果 + 可选的最终结果 JSON（如清理扫描的报告）。进度经
//! `tool://progress`（payload 为 `ToolProgress`）与 `tool://state`
//! （payload 为 `{kind, running}`）事件推给前端，也可轮询 `get_tool_control`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;

/// 进度快照。ok/skipped/failed 的含义随任务而异
/// （NCM：转换/跳过/失败；修复：修复/跳过/失败；清理扫描：已识别/重复组/未识别）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolProgress {
    pub kind: String,
    pub current: usize,
    pub total: usize,
    pub done: bool,
    pub canceled: bool,
    pub paused: bool,
    pub current_file: String,
    pub ok: usize,
    pub skipped: usize,
    pub failed: usize,
}

/// 逐文件结果（与 repair::RepairItemResult 同形，框架层复用）。
pub type TaskItem = crate::core::repair::RepairItemResult;

#[derive(Clone)]
pub struct ToolTaskState {
    paused: Arc<AtomicBool>,
    pub kind: &'static str,
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<ToolProgress>>,
    items: Arc<Mutex<Vec<TaskItem>>>,
    /// 任务结束时放置的最终结果（如清理扫描的完整报告 JSON）。
    result: Arc<Mutex<Option<serde_json::Value>>>,
}

/// 任务开始时占位成功与否（已有同类任务在跑则 false）。守卫析构时自动释放运行位，
/// 把守卫 move 进后台任务即可，无需手动清理。
pub struct StartGuard {
    state: Arc<AtomicBool>,
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        self.state.store(false, Ordering::SeqCst);
    }
}

impl ToolTaskState {
    pub fn new(kind: &'static str) -> Self {
        Self {
            kind,
            paused: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(Mutex::new(ToolProgress::default())),
            items: Arc::new(Mutex::new(Vec::new())),
            result: Arc::new(Mutex::new(None)),
        }
    }

    /// 尝试占用运行位；已有同类任务在跑时返回 None。
    pub fn try_start(&self) -> Option<StartGuard> {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return None;
        }
        Some(StartGuard {
            state: self.running.clone(),
        })
    }

    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn paused_flag(&self) -> Arc<AtomicBool> {
        self.paused.clone()
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn set_total(&self, total: usize) {
        self.progress.lock().unwrap().total = total;
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
        let mut progress = self.progress.lock().unwrap();
        progress.paused = true;
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        let mut progress = self.progress.lock().unwrap();
        progress.paused = false;
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub fn reset(&self, total: usize) {
        self.cancel.store(false, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
        *self.items.lock().unwrap() = Vec::new();
        *self.result.lock().unwrap() = None;
        *self.progress.lock().unwrap() = ToolProgress {
            kind: self.kind.to_string(),
            total,
            paused: false,
            ..Default::default()
        };
    }

    pub fn begin_file(&self, file: &str) {
        let mut progress = self.progress.lock().unwrap();
        progress.current_file = file.to_string();
    }

    /// 记录一个文件的结果并推进计数。
    pub fn finish_item(&self, item: TaskItem) {
        let mut progress = self.progress.lock().unwrap();
        match item.status.as_str() {
            "repaired" | "converted" | "ok" => progress.ok += 1,
            "skipped" => progress.skipped += 1,
            _ => progress.failed += 1,
        }
        progress.current += 1;
        progress.current_file = item.source.clone();
        self.items.lock().unwrap().push(item);
    }

    /// 直接推进计数（无逐文件结果的场景，如扫描的阶段性统计）。
    pub fn bump(&self, ok: usize, skipped: usize, failed: usize, current: usize, current_file: &str) {
        let mut progress = self.progress.lock().unwrap();
        progress.ok = ok;
        progress.skipped = skipped;
        progress.failed = failed;
        progress.current = current;
        progress.current_file = current_file.to_string();
    }

    pub fn finish(&self, canceled: bool) -> ToolProgress {
        let mut progress = self.progress.lock().unwrap();
        progress.done = true;
        progress.canceled = canceled;
        progress.paused = false;
        progress.current_file = String::new();
        progress.clone()
    }

    pub fn set_result(&self, result: serde_json::Value) {
        *self.result.lock().unwrap() = Some(result);
    }

    pub fn snapshot(&self) -> (ToolProgress, Vec<TaskItem>, Option<serde_json::Value>, bool) {
        (
            self.progress.lock().unwrap().clone(),
            self.items.lock().unwrap().clone(),
            self.result.lock().unwrap().clone(),
            self.running.load(Ordering::SeqCst),
        )
    }
}

/// `get_tool_control` 返回的完整快照。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTaskSnapshot {
    pub kind: String,
    pub running: bool,
    pub progress: ToolProgress,
    pub items: Vec<TaskItem>,
    pub result: Option<serde_json::Value>,
}

/// `tool://state` 事件负载。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStateEvent {
    pub kind: String,
    pub running: bool,
}
