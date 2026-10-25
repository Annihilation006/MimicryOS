//! VMA（虚拟内存区域）管理 —— 有序单链表实现
//! 维护：homo-ai (Hugo_YuGuo)
//!
//! - 半开区间 [start, end)，按 start 升序串联
//! - 节点来自调用方提供的静态池（内核侧 .bss 数组），
//!   零动态分配依赖；池耗尽时 mmap 返回 None
//! - 接口语义对齐 mmap/munmap 的内核侧簿记：
//!     mmap(len, flags)  → 在第一个容得下的空洞自底向上切一块
//!     munmap(start,len) → 覆盖式删除，自动分裂/裁剪相邻区
//! - 演进方向：待对象分配稳定后换 BTreeMap（设计文档 02-memory.md），
//!   当前规模（百级节点）链表足够且更利于裸机审查

use core::ptr::null_mut;

pub const PAGE_SIZE: u64 = 4096;

#[derive(Clone, Copy)]
pub struct VmaNode {
    pub start: u64,
    pub end: u64, // 半开区间
    pub flags: u64,
    next: *mut VmaNode,
}

impl VmaNode {
    pub const EMPTY: VmaNode = VmaNode { start: 0, end: 0, flags: 0, next: null_mut() };
}

pub struct VmaManager {
    head: *mut VmaNode, // 已分配区，按 start 升序
    free: *mut VmaNode, // 空闲节点池
    arena_start: u64,
    arena_end: u64,
    used: usize,
    free_cnt: usize,
}

impl VmaManager {
    /// pool 指向的 pool_len 个节点收编为空闲池。
    pub fn new(pool: *mut VmaNode, pool_len: usize, arena_start: u64, arena_end: u64) -> Self {
        let mut m = VmaManager {
            head: null_mut(),
            free: null_mut(),
            arena_start,
            arena_end,
            used: 0,
            free_cnt: 0,
        };
        unsafe {
            for i in 0..pool_len {
                let n = pool.add(i);
                (*n) = VmaNode::EMPTY;
                (*n).next = m.free;
                m.free = n;
            }
            m.free_cnt = pool_len;
        }
        m
    }

    pub fn used_count(&self) -> usize { self.used }
    pub fn free_node_count(&self) -> usize { self.free_cnt }

    fn pop_free(&mut self) -> *mut VmaNode {
        let n = self.free;
        if !n.is_null() {
            self.free = unsafe { (*n).next };
            self.free_cnt -= 1;
        }
        n
    }

    fn push_free(&mut self, n: *mut VmaNode) {
        unsafe { (*n).next = self.free; }
        self.free = n;
        self.free_cnt += 1;
    }

    fn round_up_page(v: u64) -> u64 { (v + PAGE_SIZE - 1) & !(PAGE_SIZE - 1) }

    /// 在第一个能容纳的空洞里自底向上切一块；len 向上取整到页。
    pub fn mmap(&mut self, len: u64, flags: u64) -> Option<u64> {
        let len = Self::round_up_page(len);
        if len == 0 {
            return None;
        }
        unsafe {
            let mut prev: *mut VmaNode = null_mut();
            let mut cur = self.head;
            let mut gap_start = self.arena_start;
            loop {
                if gap_start.saturating_add(len) > self.arena_end {
                    return None;
                }
                let gap_end = if cur.is_null() { self.arena_end } else { (*cur).start };
                if gap_end >= gap_start && gap_end - gap_start >= len {
                    let node = self.pop_free();
                    if node.is_null() {
                        return None; // 节点池耗尽
                    }
                    (*node).start = gap_start;
                    (*node).end = gap_start + len;
                    (*node).flags = flags;
                    (*node).next = cur;
                    if prev.is_null() {
                        self.head = node;
                    } else {
                        (*prev).next = node;
                    }
                    self.used += 1;
                    return Some(gap_start);
                }
                if cur.is_null() {
                    return None;
                }
                gap_start = gap_start.max((*cur).end);
                prev = cur;
                cur = (*cur).next;
            }
        }
    }

    /// 覆盖式取消映射：与 [start, start+len) 相交的区域被删除/裁剪/分裂。
    /// 返回是否发生了任何变化。
    pub fn munmap(&mut self, start: u64, len: u64) -> bool {
        let start = Self::round_up_page(start);
        let end = start.saturating_add(Self::round_up_page(len));
        if len == 0 || start >= end || start >= self.arena_end || end <= self.arena_start {
            return false;
        }
        let req_start = start.max(self.arena_start);
        let req_end = end.min(self.arena_end);
        let mut changed = false;
        unsafe {
            let mut prev: *mut VmaNode = null_mut();
            let mut cur = self.head;
            while !cur.is_null() {
                let nxt = (*cur).next;
                let (s, e) = ((*cur).start, (*cur).end);
                if e <= req_start || s >= req_end {
                    // 不相交，继续
                    prev = cur;
                    cur = nxt;
                    continue;
                }
                changed = true;
                let left = s < req_start;   // 左侧有残留
                let right = e > req_end;    // 右侧有残留
                if left && right {
                    // 中间被掏：裁左 + 新节点接右
                    let node = self.pop_free();
                    if !node.is_null() {
                        (*node).start = req_end;
                        (*node).end = e;
                        (*node).flags = (*cur).flags;
                        (*node).next = nxt;
                        (*cur).next = node;
                        self.used += 1;
                    } else {
                        // 池耗尽的极端情况：保左弃右（保守选择）
                    }
                    (*cur).end = req_start;
                } else if left {
                    (*cur).end = req_start;
                } else if right {
                    (*cur).start = req_end;
                } else {
                    // 整块吞掉：摘链归还
                    if prev.is_null() {
                        self.head = nxt;
                    } else {
                        (*prev).next = nxt;
                    }
                    self.push_free(cur);
                    self.used -= 1;
                    cur = nxt;
                    continue;
                }
                prev = cur;
                cur = nxt;
            }
        }
        changed
    }

    /// 查询地址落在哪个区域，返回 (start, end, flags)。
    pub fn find(&self, addr: u64) -> Option<(u64, u64, u64)> {
        let mut cur = self.head;
        while !cur.is_null() {
            unsafe {
                if addr >= (*cur).start && addr < (*cur).end {
                    return Some(((*cur).start, (*cur).end, (*cur).flags));
                }
                cur = (*cur).next;
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mgr(n: usize) -> (Vec<VmaNode>, VmaManager) {
        let mut pool = vec![VmaNode::EMPTY; n];
        let m = VmaManager::new(pool.as_mut_ptr(), n, 0x1000_0000, 0x2000_0000);
        (pool, m)
    }

    #[test]
    fn t01_mmap_ascending_and_adjacent() {
        let (_p, mut m) = mgr(8);
        let a = m.mmap(4096, 1).unwrap();
        let b = m.mmap(8192, 1).unwrap();
        assert_eq!(a, 0x1000_0000);
        assert_eq!(b, 0x1000_1000); // 紧贴上一个结尾
        assert_eq!(m.used_count(), 2);
    }

    #[test]
    fn t02_len_rounds_up_to_page() {
        let (_p, mut m) = mgr(8);
        let a = m.mmap(1, 0).unwrap();
        let b = m.mmap(1, 0).unwrap();
        assert_eq!(b - a, 4096);
    }

    #[test]
    fn t03_gap_reuse() {
        let (_p, mut m) = mgr(8);
        let a = m.mmap(4096, 0).unwrap();
        let b = m.mmap(4096, 0).unwrap();
        let c = m.mmap(4096, 0).unwrap();
        assert!(m.munmap(b, 4096));
        assert_eq!(m.find(b), None);
        let d = m.mmap(4096, 0).unwrap();
        assert_eq!(d, b); // 空洞优先复用
        let _ = (a, c);
    }

    #[test]
    fn t04_munmap_splits_middle() {
        let (_p, mut m) = mgr(8);
        let base = m.mmap(4096 * 4, 0).unwrap(); // 一整块 4 页
        assert!(m.munmap(base + 4096, 4096 * 2)); // 掏空中间 2 页
        assert_eq!(m.used_count(), 2);
        assert_eq!(m.find(base), Some((base, base + 4096, 0)));
        assert_eq!(m.find(base + 4096), None);
        assert_eq!(m.find(base + 4096 * 2), None);
        assert_eq!(m.find(base + 4096 * 3), Some((base + 4096 * 3, base + 4096 * 4, 0)));
    }

    #[test]
    fn t05_pool_exhaustion() {
        let (_p, mut m) = mgr(2);
        assert!(m.mmap(4096, 0).is_some());
        assert!(m.mmap(4096, 0).is_some());
        assert_eq!(m.mmap(4096, 0), None); // 节点池没了
        assert_eq!(m.free_node_count(), 0);
    }

    #[test]
    fn t06_arena_bounds() {
        let (_p, mut m) = mgr(8);
        // 竞技场 0x1000_0000..0x2000_0000 = 0x1000_0000 字节 = 1M 页
        let huge = m.mmap(0x1000_0000, 0).unwrap();
        assert_eq!(huge, 0x1000_0000);
        assert_eq!(m.mmap(4096, 0), None); // 越界
    }

    #[test]
    fn t07_munmap_no_overlap_noop() {
        let (_p, mut m) = mgr(4);
        let a = m.mmap(4096, 0).unwrap();
        assert!(!m.munmap(a + 0x1000_0000, 4096)); // 竞技场外
        assert!(!m.munmap(a, 0)); // 零长度
        assert_eq!(m.used_count(), 1);
    }
}
