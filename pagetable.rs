//! x86_64 四级页表操作 —— 映射/解除/查询
//! 维护：homo-ai (Hugo_YuGuo)
//!
//! - 在给定 PML4 物理基址上按需创建中间级表（PDP/PD/PT），
//!   新表来自 FrameSource 并整页清零
//! - 不处理大页拆分（遇到 HUGE 挡路返回 AlreadyMapped）
//! - 不自动回收变空的中间表（簿记成本 > 收益，此规模不值得）
//! - TLB 一致性归调用方：unmap 后请调 asm_invlpg(vaddr)
//!
//! 物理地址访问经 FrameSource::frame_to_virt 换算，宿主测试用
//! 假竞技场即可全量验证本模块逻辑。

use crate::frame::FrameSource;

// ---- 页表项位 ----
pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;
pub const PTE_ACCESSED: u64 = 1 << 5;
pub const PTE_DIRTY: u64 = 1 << 6;
pub const PTE_HUGE: u64 = 1 << 7;
pub const PTE_NO_EXEC: u64 = 1 << 63;

/// 页表项里的物理地址掩码（bit 12..51）
pub const PHYS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

#[derive(Debug, PartialEq, Eq)]
pub enum MapError {
    Misaligned,   // vaddr/paddr 未按 4KiB 对齐
    FrameExhausted, // 分配中间级表时物理帧耗尽
    AlreadyMapped,  // 目标 PTE 已存在（或被大页挡住）
    NotMapped,      // 解除映射时该项不存在
}

fn indices_of(vaddr: u64) -> [usize; 4] {
    [
        ((vaddr >> 39) & 0o777) as usize,
        ((vaddr >> 30) & 0o777) as usize,
        ((vaddr >> 21) & 0o777) as usize,
        ((vaddr >> 12) & 0o777) as usize,
    ]
}

/// 建立 vaddr → paddr 的 4KiB 映射；中间级表按需分配。
pub unsafe fn map_page(
    fs: &mut dyn FrameSource,
    pml4_phys: u64,
    vaddr: u64,
    paddr: u64,
    flags: u64,
) -> Result<(), MapError> {
    if vaddr & 0xFFF != 0 || paddr & 0xFFF != 0 || pml4_phys & 0xFFF != 0 {
        return Err(MapError::Misaligned);
    }
    let idx = indices_of(vaddr);
    let mut table_phys = pml4_phys;
    for level in 0..3 {
        let table = fs.frame_to_virt(table_phys) as *mut u64;
        let mut entry = *table.add(idx[level]);
        if entry & PTE_PRESENT == 0 {
            let new_phys = fs.alloc_frame().ok_or(MapError::FrameExhausted)?;
            let new_virt = fs.frame_to_virt(new_phys) as *mut u8;
            core::ptr::write_bytes(new_virt, 0, 4096);
            // 中间级表权限：可写；用户位跟随最终映射，保证用户页可达
            entry = new_phys | PTE_PRESENT | PTE_WRITABLE | (flags & PTE_USER);
            *table.add(idx[level]) = entry;
        } else if entry & PTE_HUGE != 0 {
            return Err(MapError::AlreadyMapped);
        }
        table_phys = entry & PHYS_MASK;
    }
    let pt = fs.frame_to_virt(table_phys) as *mut u64;
    let slot = pt.add(idx[3]);
    if *slot & PTE_PRESENT != 0 {
        return Err(MapError::AlreadyMapped);
    }
    *slot = (paddr & PHYS_MASK) | PTE_PRESENT | (flags & !(PHYS_MASK));
    Ok(())
}

/// 解除 vaddr 的 4KiB 映射（不清 A/D 位以外的簿记；TLB 归调用方）。
pub unsafe fn unmap_page(
    fs: &mut dyn FrameSource,
    pml4_phys: u64,
    vaddr: u64,
) -> Result<(), MapError> {
    let idx = indices_of(vaddr);
    let mut table_phys = pml4_phys;
    for level in 0..3 {
        let table = fs.frame_to_virt(table_phys) as *mut u64;
        let entry = *table.add(idx[level]);
        if entry & PTE_PRESENT == 0 {
            return Err(MapError::NotMapped);
        }
        if entry & PTE_HUGE != 0 {
            return Err(MapError::AlreadyMapped); // 大页不在此模块管辖
        }
        table_phys = entry & PHYS_MASK;
    }
    let pt = fs.frame_to_virt(table_phys) as *mut u64;
    let slot = pt.add(idx[3]);
    if *slot & PTE_PRESENT == 0 {
        return Err(MapError::NotMapped);
    }
    *slot = 0;
    Ok(())
}

/// 查询 vaddr 当前映射的物理地址；未映射或遇到大页返回 None。
pub unsafe fn translate(
    fs: &mut dyn FrameSource,
    pml4_phys: u64,
    vaddr: u64,
) -> Option<u64> {
    let idx = indices_of(vaddr);
    let mut table_phys = pml4_phys;
    for level in 0..3 {
        let table = fs.frame_to_virt(table_phys) as *mut u64;
        let entry = *table.add(idx[level]);
        if entry & PTE_PRESENT == 0 {
            return None;
        }
        if entry & PTE_HUGE != 0 {
            return None; // 大页暂不支持精确反解
        }
        table_phys = entry & PHYS_MASK;
    }
    let pt = fs.frame_to_virt(table_phys) as *mut u64;
    let pte = *pt.add(idx[3]);
    if pte & PTE_PRESENT == 0 {
        return None;
    }
    Some(pte & PHYS_MASK)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::FrameSource;

    /// 假物理内存竞技场：帧 = 竞技场内 4096 偏移
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

    #[test]
    fn t01_map_translate_roundtrip() {
        let mut fs = Arena::new(16);
        let pml4 = fs.alloc_frame().unwrap(); // 0 号帧作根表
        let v = 0x0000_4821_2345_6000u64;    // 任意规范形用户地址（对齐）
        let p = 0x0000_0000_1234_5000u64;
        unsafe {
            assert_eq!(map_page(&mut fs, pml4, v, p, PTE_WRITABLE | PTE_USER), Ok(()));
            assert_eq!(translate(&mut fs, pml4, v), Some(p));
            // 同一虚拟地址重复映射必须被拒
            assert_eq!(map_page(&mut fs, pml4, v, p + 4096, 0), Err(MapError::AlreadyMapped));
        }
    }

    #[test]
    fn t02_unmap_then_remap() {
        let mut fs = Arena::new(16);
        let pml4 = fs.alloc_frame().unwrap();
        let v = 0x0000_07FF_FFFF_F000u64;
        unsafe {
            map_page(&mut fs, pml4, v, 0x9000, PTE_WRITABLE).unwrap();
            assert_eq!(unmap_page(&mut fs, pml4, v), Ok(()));
            assert_eq!(translate(&mut fs, pml4, v), None);
            assert_eq!(unmap_page(&mut fs, pml4, v), Err(MapError::NotMapped));
            assert_eq!(map_page(&mut fs, pml4, v, 0xB000, 0), Ok(()));
            assert_eq!(translate(&mut fs, pml4, v), Some(0xB000));
        }
    }

    #[test]
    fn t03_misaligned_rejected() {
        let mut fs = Arena::new(8);
        let pml4 = fs.alloc_frame().unwrap();
        unsafe {
            assert_eq!(map_page(&mut fs, pml4, 0x1001, 0x2000, 0), Err(MapError::Misaligned));
            assert_eq!(map_page(&mut fs, pml4, 0x1000, 0x2001, 0), Err(MapError::Misaligned));
        }
    }

    #[test]
    fn t04_translate_unmapped() {
        let mut fs = Arena::new(8);
        let pml4 = fs.alloc_frame().unwrap(); // 全零表
        unsafe { assert_eq!(translate(&mut fs, pml4, 0x5000), None); }
    }

    #[test]
    fn t05_frame_exhaustion() {
        let mut fs = Arena::new(1); // 只有根表，无后续帧
        let pml4 = fs.alloc_frame().unwrap();
        unsafe {
            assert_eq!(
                map_page(&mut fs, pml4, 0x1000, 0x2000, 0),
                Err(MapError::FrameExhausted)
            );
        }
    }

    #[test]
    fn t06_many_pages_share_tables() {
        let mut fs = Arena::new(64);
        let pml4 = fs.alloc_frame().unwrap();
        let base = 0x0000_3000_0000_0000u64;
        unsafe {
            for i in 0..64u64 {
                let v = base + i * 4096;
                assert_eq!(map_page(&mut fs, pml4, v, i * 4096, PTE_WRITABLE), Ok(()));
            }
            for i in 0..64u64 {
                assert_eq!(translate(&mut fs, pml4, base + i * 4096), Some(i * 4096));
            }
        }
    }
}
