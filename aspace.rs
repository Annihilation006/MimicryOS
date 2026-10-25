// 用户地址空间：模板 + fork 深拷贝 + COW 计数（深拷贝为主，COW 备着）
use crate::frame::FrameSource;
use crate::pagetable::{map_page, PHYS_MASK, PTE_PRESENT, PTE_HUGE};

// 引用计数，帧号直接当下标。1M 帧封顶，超了就随缘
static mut REFC: [u32; 1 << 20] = [0; 1 << 20];

pub fn refc(f: u64) -> u32 {
    unsafe { REFC[(f >> 12) as usize & 0xFFFFF] }
}
pub unsafe fn refc_inc(f: u64) -> u32 {
    let i = (f >> 12) as usize & 0xFFFFF;
    REFC[i] = REFC[i].saturating_add(1);
    REFC[i]
}
pub unsafe fn refc_dec(f: u64) -> u32 {
    let i = (f >> 12) as usize & 0xFFFFF;
    if REFC[i] > 0 {
        REFC[i] -= 1;
    }
    REFC[i]
}

// 空用户空间：内核半区共享，用户半区全空
pub unsafe fn new_user(fs: &mut dyn FrameSource, kern_pml4: u64) -> Option<u64> {
    let np = fs.alloc_frame()?;
    core::ptr::write_bytes(fs.frame_to_virt(np), 0, 4096);
    let k = fs.frame_to_virt(kern_pml4 & PHYS_MASK) as *const u64;
    let n = fs.frame_to_virt(np) as *mut u64;
    for i in 256..512 {
        *n.add(i) = *k.add(i);
    }
    Some(np)
}

// fork：src 用户半区（pml4 0..256）逐页深拷。返回拷了多少页
pub unsafe fn fork_deep(fs: &mut dyn FrameSource, src: u64, dst: u64) -> usize {
    let mut cnt = 0;
    for pi in 0..256usize {
        let pdpt_e = *(fs.frame_to_virt(src & PHYS_MASK) as *const u64).add(pi);
        if pdpt_e & PTE_PRESENT == 0 || pdpt_e & PTE_HUGE != 0 {
            continue;
        }
        for pdi in 0..512usize {
            let pd_e = *(fs.frame_to_virt(pdpt_e & PHYS_MASK) as *const u64).add(pdi);
            if pd_e & PTE_PRESENT == 0 || pd_e & PTE_HUGE != 0 {
                continue;
            }
            for pti in 0..512usize {
                let pt_e = *(fs.frame_to_virt(pd_e & PHYS_MASK) as *const u64).add(pti);
                if pt_e & PTE_PRESENT == 0 || pt_e & PTE_HUGE != 0 {
                    continue;
                }
                let v = ((pi as u64) << 39) | ((pdi as u64) << 30)
                    | ((pti as u64) << 21) | 0; // pte 里再取偏移
                for off in 0..512u64 {
                    let pte = *(fs.frame_to_virt(pt_e & PHYS_MASK) as *const u64).add(off as usize);
                    if pte & PTE_PRESENT == 0 {
                        continue;
                    }
                    let sp = pte & PHYS_MASK;
                    let newp = match fs.alloc_frame() {
                        Some(p) => p,
                        None => return cnt, // 帧尽了就到这，不装了
                    };
                    core::ptr::copy(
                        fs.frame_to_virt(sp),
                        fs.frame_to_virt(newp),
                        4096,
                    );
                    let va = ((pi as u64) << 39) | ((pdi as u64) << 30)
                        | ((pti as u64) << 21) | (off << 12);
                    let flags = pt_e & !PHYS_MASK & !1; // 掩码里留 flags，PRESENT map 自己置
                    let _ = v;
                    if map_page(fs, dst, va, newp, flags).is_ok() {
                        cnt += 1;
                    }
                }
            }
        }
    }
    cnt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameAllocator;
    use crate::pagetable::{translate, PTE_WRITABLE, PTE_USER};

    struct A {
        mem: Vec<u8>,
        n: usize,
    }
    impl A {
        fn new(f: usize) -> A {
            A { mem: vec![0u8; f * 4096], n: 0 }
        }
    }
    impl FrameSource for A {
        fn alloc_frame(&mut self) -> Option<u64> {
            if self.n + 4096 <= self.mem.len() {
                let o = self.n;
                self.n += 4096;
                Some(o as u64)
            } else {
                None
            }
        }
        fn free_frame(&mut self, _p: u64) {}
        fn frame_to_virt(&self, p: u64) -> *mut u8 {
            unsafe { self.mem.as_ptr().add(p as usize) as *mut u8 }
        }
    }

    #[test]
    fn t1_refc() {
        unsafe {
            assert_eq!(refc(0x123000), 0);
            refc_inc(0x123000);
            refc_inc(0x123000);
            assert_eq!(refc(0x123000), 2);
            refc_dec(0x123000);
            refc_dec(0x123000);
            assert_eq!(refc(0x123000), 0); // 不许负
        }
    }

    #[test]
    fn t2_new_user_shares_kernel_half() {
        let mut a = A::new(32);
        let kp = a.alloc_frame().unwrap();
        unsafe {
            *(a.frame_to_virt(kp) as *mut u64).add(511) = 0xDEAD | 1;
            *(a.frame_to_virt(kp) as *mut u64).add(300) = 0xBEEF | 1;
        }
        let up = unsafe { new_user(&mut a, kp) }.unwrap();
        unsafe {
            assert_eq!(*(a.frame_to_virt(up) as *const u64).add(511), 0xDEAD | 1);
            assert_eq!(*(a.frame_to_virt(up) as *const u64).add(300), 0xBEEF | 1);
            assert_eq!(*(a.frame_to_virt(up) as *const u64).add(0), 0);
        }
    }

    #[test]
    fn t3_fork_deep_copies() {
        // 全程只用一个分配器，两个会发同样的帧号
        let mut a = A::new(128);
        let src = a.alloc_frame().unwrap();
        let dst = unsafe { new_user(&mut a, src) }.unwrap();
        let u1 = 0x0000_0000_1000_0000u64;
        let u2 = 0x0000_0000_2000_0000u64;
        unsafe {
            map_page(&mut a, src, u1, 0x5000, PTE_WRITABLE | PTE_USER).unwrap();
            map_page(&mut a, src, u2, 0x6000, PTE_WRITABLE | PTE_USER).unwrap();
            *(a.frame_to_virt(0x5000) as *mut u64) = 0x1111;
            *(a.frame_to_virt(0x6000) as *mut u64) = 0x2222;
        }
        let n = unsafe { fork_deep(&mut a, src, dst) };
        assert_eq!(n, 2);
        unsafe {
            // 父改自己的，子不该变
            *(a.frame_to_virt(0x5000) as *mut u64) = 0x3333;
            let cp = translate(&mut a, dst, u1).unwrap();
            assert_ne!(cp, 0x5000, "得是新帧不是共享");
            assert_eq!(*(a.frame_to_virt(cp) as *mut u64), 0x1111);
        }
    }
}
