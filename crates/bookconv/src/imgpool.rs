//! 图片并行处理的内存预算与线程数。
//!
//! 图片逐张独立，可以并行（漫画一卷几百页，缩放和编码是大头）。但同时在处理的图片总像素受 [`PIXEL_BUDGET`]
//! 限制——每张图开工前按头部声明的像素数申请额度，额度不够就等；单张图超过总预算时独占全部额度（此时不与别的图
//! 并行）。并行不改变任何一张图的处理结果（每张仍是同一个纯函数），输出按原条目顺序写，逐字节可复现。

use std::sync::{Condvar, Mutex};

/// 同时在处理的图片总像素上限：4 张文字书插图解码上限（[`crate::imgopt::MAX_DECODE_PIXELS`]，900 万像素）＝ 3600 万像素。
/// 实测整页处理约 9–16MB/百万像素（见 `MAX_DECODE_PIXELS` 文档），最坏情况（几张接近上限的超大图同时处理）峰值约
/// 350–580MB，对电脑端足够安全；典型漫画页 100–200 万像素，[`worker_count`] 个线程可以全部同时开工（8 × 200 万 ＝
/// 1600 万，远低于上限）。比它还大的漫画页（最大 [`crate::imgopt::MAX_COMIC_DECODE_PIXELS`]）开工时独占全部额度：
/// 等手上的图都做完才开始，做完之前别的图也不开工——大页一张一张来，峰值内存就是单张大页的量。
pub const PIXEL_BUDGET: u64 = 4 * crate::imgopt::MAX_DECODE_PIXELS;

/// 并行工作线程数上限。再往上加，写 zip（单线程、按顺序）和读原图会成为瓶颈，只多占内存不再明显加速。
const MAX_WORKERS: usize = 8;

/// 并行工作线程数：取 CPU 核数，封顶 [`MAX_WORKERS`]。
pub fn worker_count() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).clamp(1, MAX_WORKERS)
}

pub struct PixelBudget {
    cap: u64,
    used: Mutex<u64>,
    cv: Condvar,
}

pub struct Permit<'a> {
    budget: &'a PixelBudget,
    n: u64,
}

impl PixelBudget {
    pub fn new(cap: u64) -> PixelBudget {
        PixelBudget { cap, used: Mutex::new(0), cv: Condvar::new() }
    }

    /// 申请 `px` 像素的额度（超过总预算按总预算算，即独占）。阻塞到有额度；返回的 [`Permit`] 析构时归还。
    pub fn acquire(&self, px: u64) -> Permit<'_> {
        let need = px.clamp(1, self.cap);
        let mut used = self.used.lock().unwrap_or_else(|e| e.into_inner());
        while *used + need > self.cap {
            used = self.cv.wait(used).unwrap_or_else(|e| e.into_inner());
        }
        *used += need;
        Permit { budget: self, n: need }
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut used = self.budget.used.lock().unwrap_or_else(|e| e.into_inner());
        *used -= self.n;
        self.budget.cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    #[test]
    fn budget_never_exceeds_cap_under_contention() {
        let b = Arc::new(PixelBudget::new(1000));
        let (cur, peak) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let hs: Vec<_> = (0..8)
            .map(|_| {
                let (b, cur, peak) = (b.clone(), cur.clone(), peak.clone());
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        let _p = b.acquire(400);
                        let now = cur.fetch_add(400, Ordering::SeqCst) + 400;
                        peak.fetch_max(now, Ordering::SeqCst);
                        std::thread::yield_now();
                        cur.fetch_sub(400, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        hs.into_iter().for_each(|h| h.join().unwrap());
        assert!(peak.load(Ordering::SeqCst) <= 1000, "同时占用的像素超过了预算: {}", peak.load(Ordering::SeqCst));
    }

    #[test]
    fn oversized_image_takes_whole_budget_and_still_completes() {
        let b = PixelBudget::new(100);
        let p = b.acquire(10_000); // 超过总预算：独占，不死锁
        drop(p);
        let _q = b.acquire(100);
    }

    #[test]
    fn worker_count_is_between_1_and_max() {
        assert!((1..=MAX_WORKERS).contains(&worker_count()));
    }
}
