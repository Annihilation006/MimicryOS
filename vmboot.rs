// 换页表用。抄 BOOTBOOT 高半区 + 铺直映射，然后 CR3 一踢走人
use crate::frame::{FrameSource, DIRECT_BASE};
use crate::pagetable::PHYS_MASK;

unsafe fn zp(fs: &mut dyn FrameSource, p: u64) {
    core::ptr::write_bytes(fs.frame_to_virt(p), 0, 4096);
}

// 返回新 pml4 物理址，不动 CR3（踢的动作归调用方）
pub unsafe fn build(fs: &mut dyn FrameSource, old_cr3: u64, max_phys: u64) -> Option<u64> {
    let np = fs.alloc_frame()?;
    zp(fs, np);
    let old = fs.frame_to_virt(old_cr3 & PHYS_MASK) as *const u64;
    let nu = fs.frame_to_virt(np) as *mut u64;
    for i in 256..512 {
        *nu.add(i) = *old.add(i); // 高半区原样搬，内核/fb/bootboot 都在里面
    }

    // 直映射 2M 大页，slot 自己算别抄魔数
    let slot = (DIRECT_BASE >> 39) as usize & 511;
    let pdpt = fs.alloc_frame()?;
    zp(fs, pdpt);
    let gigs = ((max_phys + 0x3FFF_FFFF) / 0x4000_0000).max(1);
    for g in 0..gigs {
        let pd = fs.alloc_frame()?;
        zp(fs, pd);
        let pdv = fs.frame_to_virt(pd) as *mut u64;
        for j in 0..512u64 {
            let pa = (g << 30) | (j << 21);
            if pa >= max_phys {
                break;
            }
            *pdv.add(j as usize) = pa | 1 | 2 | (1 << 7); // P|W|PS
        }
        *(fs.frame_to_virt(pdpt) as *mut u64).add(g as usize) = pd | 1 | 2;
    }
    *nu.add(slot) = pdpt | 1 | 2;
    Some(np)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameAllocator;

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

    fn alloc_wrap(n: usize) -> (Vec<u64>, FrameAllocator) {
        let w = (n + 63) / 64;
        let mut bm = vec![0u64; w];
        let p = bm.as_mut_ptr();
        (bm, unsafe { FrameAllocator::from_raw(p, w, n) })
    }

    #[test]
    fn t1_shape() {
        let mut a = A::new(64);
        let oldp = a.alloc_frame().unwrap(); // 假旧表占 frame0，别让 build 撞上
        unsafe {
            *(a.frame_to_virt(oldp) as *mut u64).add(511) = 0x1000 | 1;
            *(a.frame_to_virt(oldp) as *mut u64).add(510) = 0x2000 | 1;
        }
        let np = unsafe { build(&mut a, oldp, 1 << 30) }.unwrap();
        let nu = a.frame_to_virt(np) as *const u64;
        unsafe {
            assert_eq!(*nu.add(511), 0x1000 | 1, "高半区没搬过来");
            assert_eq!(*nu.add(510), 0x2000 | 1);
            let slot = (DIRECT_BASE >> 39) as usize & 511;
            let pdpt = *nu.add(slot) & PHYS_MASK;
            let pd = *(a.frame_to_virt(pdpt) as *const u64) & PHYS_MASK;
            let e0 = *(a.frame_to_virt(pd) as *const u64);
            assert_eq!(e0 & PHYS_MASK, 0, "直映射第一页该是物理 0");
            assert!(e0 & 1 != 0 && e0 & 2 != 0 && e0 & (1 << 7) != 0);
            // 1GiB 就该铺 512 个 2M 页
            let e511 = *(a.frame_to_virt(pd) as *const u64).add(511);
            assert_eq!(e511 & PHYS_MASK, (511u64 << 21));
        }
    }

    #[test]
    fn t2_oob_gig_skipped() {
        let mut a = A::new(64);
        let oldp = a.alloc_frame().unwrap();
        let np = unsafe { build(&mut a, oldp, 0x4000_0000 + 0x200000) }.unwrap(); // 1G+2M
        let slot = (DIRECT_BASE >> 39) as usize & 511;
        unsafe {
            let pdpt = *(a.frame_to_virt(np) as *const u64).add(slot) & PHYS_MASK;
            let pd1 = *(a.frame_to_virt(pdpt) as *const u64).add(1) & PHYS_MASK;
            assert!(pd1 != 0, "第二个 GiB 该有自己的 pd");
            let e1 = *(a.frame_to_virt(pd1) as *const u64).add(1);
            assert_eq!(e1, 0, "1G+2M 往后不该铺");
        }
    }
}
