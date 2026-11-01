//! 目录扫描工作池：LIFO 任务栈 + 完成计数，供并行扫描复用。
//!
//! 设计要点（取代 rayon 的原因）：worker 从不阻塞等待子任务结果——
//! 任务完成后由完成者负责聚合/级联，因此不存在"所有线程都在等
//! 队列里尚未启动的任务"的经典死锁；取消语义也简单（pop 返回 None，
//! 剩余任务按空操作快速排干）。

use std::{
    sync::{
        Condvar, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub struct JobPool<J> {
    /// LIFO 栈：同目录的子任务优先被同一线程拿到（路径局部性更好）
    queue: Mutex<Vec<J>>,
    available: Condvar,
    /// 已入队尚未完成的任务数
    active: AtomicUsize,
}

impl<J> JobPool<J> {
    pub fn new() -> Self {
        Self {
            queue: Mutex::new(Vec::new()),
            available: Condvar::new(),
            active: AtomicUsize::new(0),
        }
    }

    /// 入队一个任务（active +1 并唤醒空闲 worker）。
    pub fn push(&self, job: J) {
        // 先增 active 再入队，且在同一把锁内完成：否则 worker 可能在
        // active 仍为 0 时看到空队列而退出，wait_idle 也可能提前返回。
        let mut queue = self.queue.lock().unwrap();
        self.active.fetch_add(1, Ordering::Relaxed);
        queue.push(job);
        drop(queue);
        self.available.notify_one();
    }

    /// 取一个任务；队列为空且全部完成、或已请求取消时返回 None。
    pub fn pop(&self, cancel: &AtomicBool) -> Option<J> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            // 取消优先：立即让 worker 退出，剩余任务随池一起丢弃
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            if let Some(job) = queue.pop() {
                return Some(job);
            }
            if self.active.load(Ordering::Relaxed) == 0 {
                return None;
            }
            let (next, _) = self
                .available
                .wait_timeout(queue, Duration::from_millis(50))
                .unwrap();
            queue = next;
        }
    }

    /// 标记一个任务完成；归零时唤醒所有等待者。
    pub fn done(&self) {
        if self.active.fetch_sub(1, Ordering::Relaxed) == 1 {
            self.available.notify_all();
        }
    }

    /// 等待全部任务完成；返回是否干净完成（false = 已取消）。
    pub fn wait_idle(&self, cancel: &AtomicBool) -> bool {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if self.active.load(Ordering::Relaxed) == 0 {
                return true;
            }
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            let (next, _) = self
                .available
                .wait_timeout(queue, Duration::from_millis(50))
                .unwrap();
            queue = next;
        }
    }
}

/// 扫描线程数：默认 min(8, 可用并行度)，避免扫描把桌面应用卡死。
pub fn scan_thread_count(threads: usize) -> usize {
    if threads > 0 {
        return threads.min(16);
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn pool_completes_all_pushed_jobs() {
        let pool = Arc::new(JobPool::<usize>::new());
        let cancel = AtomicBool::new(false);
        let processed = Arc::new(AtomicUsize::new(0));

        let total = 200usize;
        for value in 0..total {
            pool.push(value);
        }

        let cancel_ref = &cancel;
        std::thread::scope(|scope| {
            for _ in 0..4 {
                let pool = pool.clone();
                let processed = processed.clone();
                scope.spawn(move || {
                    while let Some(value) = pool.pop(cancel_ref) {
                        processed.fetch_add(value, Ordering::Relaxed);
                        pool.done();
                    }
                });
            }
        });

        assert_eq!(processed.load(Ordering::Relaxed), total * (total - 1) / 2);
        assert!(pool.wait_idle(&cancel));
    }

    #[test]
    fn pool_cancel_drains_quickly() {
        let pool = Arc::new(JobPool::<usize>::new());
        let cancel = AtomicBool::new(false);
        for value in 0..10_000 {
            pool.push(value);
        }
        cancel.store(true, Ordering::Relaxed);
        // pop 立即返回 None，不会逐个排干
        assert!(pool.pop(&cancel).is_none());
        assert!(pool.wait_idle(&cancel) == false);
    }
}
