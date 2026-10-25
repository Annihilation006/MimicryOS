//! 内核小对象内存池 —— K&R 风格空闲链表分配器
//! 维护：homo-ai (Hugo_YuGuo)
//!
//! - 内存来源：FrameSource 供给的 4096 字节帧（宿主测试用假竞技场）
//! - 按地址有序的 singly-linked 空闲链，free 时与物理相邻块合并
//! - 分配固定 16 字节对齐（帧本身 4096 对齐，头 16 字节，天然满足）
//! - 不做页级归还（全部空闲且恰好合并成整帧时也不还给 FrameSource，
//!   桌面级内核的用量不值得这个复杂度；有需要再加）
//!
//! 供 VMA 节点等 Rust 侧数据结构使用，避免依赖 main 分支的 kmalloc。

use crate::frame::{FrameSource, FRAME_SIZE};
use core::ptr::null_mut;

#[repr(C)]
struct Block {
    size: usize, // 含本头在内的字节数
    next: *mut Block,
}

const HDR: usize = core::mem::size_of::<Block>();

pub struct Kmem {
    head: *mut Block, // 空闲块，按地址升序
}

impl Kmem {
    pub const fn new() -> Self {
        Kmem { head: null_mut() }
    }

    /// 从 FrameSource 取 n 帧挂入池子，返回实际新增字节数。
    pub unsafe fn add_frames(&mut self, fs: &mut dyn FrameSource, n: usize) -> usize {
        let mut added = 0usize;
        for _ in 0..n {
            let phys = match fs.alloc_frame() {
                Some(p) => p,
                None => break,
            };
            let virt = fs.frame_to_virt(phys);
            core::ptr::write_bytes(virt, 0, FRAME_SIZE as usize);
            let b = virt as *mut Block;
            (*b).size = FRAME_SIZE as usize;
            (*b).next = null_mut();
            self.insert_block(b);
            added += FRAME_SIZE as usize;
        }
        added
    }

    /// 有序插入空闲块并与相邻块合并。
    unsafe fn insert_block(&mut self, b: *mut Block) {
        if self.head.is_null() || b < self.head {
            (*b).next = self.head;
            self.head = b;
        } else {
            let mut cur = self.head;
            while !(*cur).next.is_null() && (*cur).next < b {
                cur = (*cur).next;
            }
            (*b).next = (*cur).next;
            (*cur).next = b;
        }
        self.coalesce();
    }

    /// 一趟合并所有物理相邻的空闲块。
    unsafe fn coalesce(&mut self) {
        let mut cur = self.head;
        while !cur.is_null() {
            let nxt = (*cur).next;
            if !nxt.is_null() && (cur as usize + (*cur).size) == nxt as usize {
                (*cur).size += (*nxt).size;
                (*cur).next = (*nxt).next;
            } else {
                cur = nxt;
            }
        }
    }

    /// first-fit 分配（16 字节对齐）。无可用空间返回 null。
    pub unsafe fn alloc(&mut self, size: usize) -> *mut u8 {
        let need = (size + HDR + 15) & !15;
        let mut prev: *mut Block = null_mut();
        let mut cur = self.head;
        while !cur.is_null() {
            if (*cur).size >= need {
                let rem = (*cur).size - need;
                if rem >= HDR + 16 {
                    // 从块尾拆出剩余部分顶替本块在链上的位置
                    let r = (cur as *mut u8).add(need) as *mut Block;
                    (*r).size = rem;
                    (*r).next = (*cur).next;
                    (*cur).size = need;
                    if prev.is_null() {
                        self.head = r;
                    } else {
                        (*prev).next = r;
                    }
                } else {
                    // 拆不出整头，整块拿走
                    if prev.is_null() {
                        self.head = (*cur).next;
                    } else {
                        (*prev).next = (*cur).next;
                    }
                }
                return (cur as *mut u8).add(HDR) as *mut u8;
            }
            prev = cur;
            cur = (*cur).next;
        }
        null_mut()
    }

    /// 释放先前 alloc 得到的指针（大小记录在块头里）。
    pub unsafe fn free(&mut self, ptr: *mut u8) {
        if ptr.is_null() {
            return;
        }
        let blk = (ptr as *mut u8).sub(HDR) as *mut Block;
        (*blk).next = null_mut();
        self.insert_block(blk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameSource;

    struct Arena {
        mem: Vec<u8>,
        next: usize,
    }
    impl Arena {
        fn new(frames: usize) -> Arena {
            Arena { mem: vec![0u8; frames * 4096], next: 0 }
        }
    }
    impl FrameSource for Arena {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.next + 4096 <= self.mem.len() {
                let o = self.next;
                self.next += 4096;
                Some(o as u64)
            } else {
                None
            }
        }
        fn free_frame(&mut self, _phys: u64) {}
        fn frame_to_virt(&self, phys: u64) -> *mut u8 {
            unsafe { self.mem.as_ptr().add(phys as usize) as *mut u8 }
        }
    }

    fn km(frames: usize) -> (Arena, Kmem) {
        let mut a = Arena::new(frames);
        let mut k = Kmem::new();
        unsafe { k.add_frames(&mut a, frames); }
        (a, k)
    }

    #[test]
    fn t01_alloc_distinct_and_writable() {
        let (_a, mut k) = km(2);
        let mut ptrs = Vec::new();
        for i in 0..16usize {
            let p = unsafe { k.alloc(64) };
            assert!(!p.is_null(), "第 {} 次分配失败", i);
            unsafe { *(p as *mut u64) = 0xDEAD_BEEF_0000 + i as u64; }
            ptrs.push(p);
        }
        for (i, &p) in ptrs.iter().enumerate() {
            assert_eq!(unsafe { *(p as *mut u64) }, 0xDEAD_BEEF_0000 + i as u64);
        }
        // 两两不同
        for i in 0..ptrs.len() {
            for j in i + 1..ptrs.len() {
                assert_ne!(ptrs[i], ptrs[j]);
            }
        }
    }

    #[test]
    fn t02_free_coalesce_then_big_reuse() {
        let (_a, mut k) = km(1);
        let p1 = unsafe { k.alloc(64) };
        let p2 = unsafe { k.alloc(64) };
        unsafe { k.free(p1); k.free(p2); }
        // 释放后应合并回整帧，4080+头=4096 恰好放得下
        let big = unsafe { k.alloc(4080) };
        assert!(!big.is_null());
        assert_eq!(big, p1); // 从帧头重新开始
    }

    #[test]
    fn t03_exhaustion_then_add_frames() {
        let (_a, mut k) = km(1);
        let mut got = Vec::new();
        loop {
            let p = unsafe { k.alloc(4080) };
            if p.is_null() { break; }
            got.push(p);
        }
        assert_eq!(got.len(), 1); // 一帧只够一个 4080
        assert!(unsafe { k.alloc(32) }.is_null());
        // 追加一帧后恢复供给
        let mut arena = Arena::new(1);
        unsafe { k.add_frames(&mut arena, 1); }
        assert!(!unsafe { k.alloc(32) }.is_null());
    }

    #[test]
    fn t04_zero_and_small_allocs() {
        let (_a, mut k) = km(1);
        assert!(!unsafe { k.alloc(0) }.is_null());
        assert!(!unsafe { k.alloc(1) }.is_null());
        assert!(!unsafe { k.alloc(16) }.is_null());
    }
}
