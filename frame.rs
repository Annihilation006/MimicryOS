//! 物理帧位图分配器 —— MimicryOS test 分支（设计文档 02-memory.md 第一步）
//! 维护：homo-ai (Hugo_YuGuo)
//!
//! - 纯逻辑、无堆依赖：位图存放在调用方提供的内存里（内核侧为 .bss 静态数组）
//! - 1 bit = 1 个 4KiB 物理帧；**1=占用，0=空闲**（next-fit 扫描找 0 位）
//! - 初始化语义：`init_all_used()` 先置全占用，再用 `mark_free_range()`
//!   按 BOOTBOOT 内存图放开可用区——宁可漏放不可错放，杜绝把保留区发给人
//! - 并发：尚未加锁（当前单核、中断未启用）；调度器落地后由调用方加锁
//!   或在本层补 spinlock（cpu 层已有 cli/sti 可用）
//!
//! 本文件同时是内核模块（经 rust_kernel.rs 引入）与宿主单测 crate
//! （`rustc --test frame.rs`），算法先在宿主机上过断言再进内核。

pub const FRAME_SIZE: u64 = 4096;
pub const WORD_BITS: usize = 64;

pub struct FrameAllocator {
    bitmap: *mut u64,
    nwords: usize,
    total: usize,   // 管理的总帧数（尾部不足 64 的部分视为占用）
    free_cnt: usize,
    hint: usize,    // next-fit：上次命中的 word 下标
}

impl FrameAllocator {
    /// # Safety
    /// `bitmap` 必须指向至少 `nwords` 个 u64 的可写内存，且在使用期内有效。
    pub unsafe fn from_raw(bitmap: *mut u64, nwords: usize, total: usize) -> Self {
        Self { bitmap, nwords, total, free_cnt: 0, hint: 0 }
    }

    /// 置为"全部占用"。随后必须用 mark_free_range 放开可用区间。
    pub unsafe fn init_all_used(&mut self) {
        for i in 0..self.nwords {
            *self.bitmap.add(i) = !0u64;
        }
        self.free_cnt = 0;
        self.hint = 0;
    }

    pub fn total(&self) -> usize { self.total }
    pub fn free_count(&self) -> usize { self.free_cnt }

    /// 放开 [start_frame, start_frame+count) 为空闲；越界部分忽略；
    /// 重复放开不重复计数。
    pub unsafe fn mark_free_range(&mut self, start_frame: usize, count: usize) {
        let mut i = start_frame;
        let end = start_frame.saturating_add(count);
        while i < end && i < self.total {
            let (w, b) = (i / WORD_BITS, i % WORD_BITS);
            let word = *self.bitmap.add(w);
            if word & (1u64 << b) != 0 {
                *self.bitmap.add(w) = word & !(1u64 << b);
                self.free_cnt += 1;
            }
            i += 1;
        }
    }

    /// next-fit 分配一个帧，返回帧号；无空闲返回 None。
    pub unsafe fn alloc(&mut self) -> Option<usize> {
        if self.free_cnt == 0 || self.nwords == 0 {
            return None;
        }
        for step in 0..self.nwords {
            let w = (self.hint + step) % self.nwords;
            let word = *self.bitmap.add(w);
            if word != !0u64 {
                let b = word.trailing_ones() as usize; // 第一个 0 位
                let idx = w * WORD_BITS + b;
                if idx < self.total {
                    *self.bitmap.add(w) = word | (1u64 << b);
                    self.free_cnt -= 1;
                    self.hint = w;
                    return Some(idx);
                }
            }
        }
        None
    }

    /// 释放一个帧。越界或重复释放返回 false。
    pub unsafe fn free(&mut self, idx: usize) -> bool {
        if idx >= self.total {
            return false;
        }
        let (w, b) = (idx / WORD_BITS, idx % WORD_BITS);
        let word = *self.bitmap.add(w);
        if word & (1u64 << b) == 0 {
            return false; // double free
        }
        *self.bitmap.add(w) = word & !(1u64 << b);
        self.free_cnt += 1;
        if w < self.hint {
            self.hint = w; // 让提示指针回跳，避免低地址空洞饿死
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_alloc(total: usize) -> (Vec<u64>, FrameAllocator) {
        let nwords = (total + WORD_BITS - 1) / WORD_BITS;
        let mut bm = vec![0u64; nwords];
        let fa = unsafe { FrameAllocator::from_raw(bm.as_mut_ptr(), nwords, total) };
        (bm, fa)
    }

    #[test]
    fn t01_all_used_then_none() {
        let (_bm, mut fa) = new_alloc(128);
        unsafe { fa.init_all_used() };
        assert_eq!(fa.free_count(), 0);
        assert_eq!(unsafe { fa.alloc() }, None);
    }

    #[test]
    fn t02_mark_alloc_in_order() {
        let (_bm, mut fa) = new_alloc(128);
        unsafe { fa.init_all_used() };
        unsafe { fa.mark_free_range(0, 4) };
        assert_eq!(fa.free_count(), 4);
        for expect in 0..4usize {
            assert_eq!(unsafe { fa.alloc() }, Some(expect));
        }
        assert_eq!(unsafe { fa.alloc() }, None);
    }

    #[test]
    fn t03_double_free_rejected() {
        let (_bm, mut fa) = new_alloc(64);
        unsafe { fa.init_all_used() };
        unsafe { fa.mark_free_range(10, 1) };
        let got = unsafe { fa.alloc() };
        assert_eq!(got, Some(10));
        assert!(unsafe { fa.free(10) });
        assert!(!unsafe { fa.free(10) }, "double free must be rejected");
        assert!(!unsafe { fa.free(9999) }, "out of range must be rejected");
    }

    #[test]
    fn t04_free_then_reuse() {
        let (_bm, mut fa) = new_alloc(64);
        unsafe { fa.init_all_used() };
        unsafe { fa.mark_free_range(0, 8) };
        let _ = (0..8).map(|_| unsafe { fa.alloc() }).count();
        assert!(unsafe { fa.free(3) });
        assert_eq!(unsafe { fa.alloc() }, Some(3), "freed frame should be reused");
    }

    #[test]
    fn t05_tail_bits_beyond_total_stay_used() {
        // total=70，word1 只有 0..5 有效；越界位必须保持占用
        let (_bm, mut fa) = new_alloc(70);
        unsafe { fa.init_all_used() };
        unsafe { fa.mark_free_range(0, 70) };
        assert_eq!(fa.free_count(), 70);
        for _ in 0..70 {
            assert!(unsafe { fa.alloc() }.is_some());
        }
        assert_eq!(unsafe { fa.alloc() }, None);
    }

    #[test]
    fn t06_next_fit_wraps_and_avoids_starvation() {
        let (_bm, mut fa) = new_alloc(256);
        unsafe { fa.init_all_used() };
        // 只放开两个高 word 区段，hint 会随分配后移
        unsafe { fa.mark_free_range(64, 64) }; // word1 全空闲
        unsafe { fa.mark_free_range(192, 64) }; // word3 全空闲
        for _ in 0..128 {
            assert!(unsafe { fa.alloc() }.is_some());
        }
        assert_eq!(unsafe { fa.alloc() }, None);
        // 释放一个低位帧后，hint 应回跳使其可被再次分配
        assert!(unsafe { fa.free(65) });
        assert_eq!(unsafe { fa.alloc() }, Some(65));
    }

    #[test]
    fn t07_mark_free_range_clamps_and_idempotent() {
        let (_bm, mut fa) = new_alloc(100);
        unsafe { fa.init_all_used() };
        unsafe { fa.mark_free_range(90, 1000) }; // 越界截断到 90..99
        assert_eq!(fa.free_count(), 10);
        unsafe { fa.mark_free_range(90, 1000) }; // 幂等
        assert_eq!(fa.free_count(), 10);
    }
}
