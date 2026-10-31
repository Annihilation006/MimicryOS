// fat12.rs — 朋友 fat12 磁盘的翻译器后端 (Rust 薄层)
// 磁盘布局 (kernel.c 2880 起): 引导扇区 2880 · FAT 2881-2884 ·
// 根目录 2885-2889 (16 项 × 32B) · 数据区 2890 起 (单簇文件)。
// 簇 I/O 走 C 侧 ata_read/ata_write 原语 (全局符号, 直接 extern)。
// 本模块只做目录项解析与数据搬运, 不碰 fb_print——程序化可用。

use crate::translator::FsBack;

pub const DIR_LBA: u32 = 2885;
pub const DATA_LBA: u32 = 2890;
pub const SECTOR: usize = 512;
pub const NAME_LEN: usize = 11; // 8+3 空格填充

extern "C" {
    fn ata_read(lba: u32, buf: *mut u8);
    fn ata_write(lba: u32, buf: *const u8);
}

// 目录项偏移: 名 8 + 扩展 3 + 属性@11 + 首簇@26(u16) + 大小@28(u32)
fn entry_name(dir: &[u8], e: usize) -> Option<[u8; NAME_LEN]> {
    let first = unsafe { *dir.as_ptr().add(e) };
    if first == 0x00 || first == 0xE5 {
        return None; // 末尾 / 已删除
    }
    let mut name = [b' '; NAME_LEN];
    unsafe { name.copy_from_slice(core::slice::from_raw_parts(dir.as_ptr().add(e), NAME_LEN)); }
    Some(name)
}

// 请求名 (任意长) → 8+3 空格填充比较; 只取首段, 不支持多扩展名
fn name_matches(dir: &[u8], e: usize, want: &[u8]) -> bool {
    let mut wi = 0usize;
    for d in 0..8 {
        let c = if wi < want.len() && want[wi] != b'.' { want[wi] } else { b' ' };
        if unsafe { *dir.as_ptr().add(e + d) } != c {
            return false;
        }
        if wi < want.len() && want[wi] != b'.' {
            wi += 1;
        }
    }
    if wi < want.len() && want[wi] == b'.' {
        wi += 1;
    }
    for d in 0..3 {
        let c = if wi < want.len() { want[wi] } else { b' ' };
        if unsafe { *dir.as_ptr().add(e + 8 + d) } != c {
            return false;
        }
        if wi < want.len() {
            wi += 1;
        }
    }
    true
}

fn find_entry(dir: &[u8], want: &[u8]) -> Option<usize> {
    let mut e = 0usize;
    while e + 32 <= dir.len() {
        if unsafe { *dir.as_ptr().add(e) } == 0x00 {
            break; // 目录末尾
        }
        if unsafe { *dir.as_ptr().add(e) } != 0xE5 {
            if let Some(_n) = entry_name(dir, e) {
                if name_matches(dir, e, want) {
                    return Some(e);
                }
            }
        }
        e += 32;
    }
    None
}

fn free_entry(dir: &[u8]) -> Option<usize> {
    let mut e = 0usize;
    while e + 32 <= dir.len() {
        let b = unsafe { *dir.as_ptr().add(e) };
        if b == 0x00 || b == 0xE5 {
            return Some(e);
        }
        e += 32;
    }
    None
}

// u16 小端读 (裸指针: 运行期索引的越界检查符号裸机链接不了;
// 调用方保证 off+1 < buf.len(), 目录项偏移全部为编译期常量)
fn rd16(buf: &[u8], off: usize) -> usize {
    unsafe {
        *buf.as_ptr().add(off) as usize | ((*buf.as_ptr().add(off + 1) as usize) << 8)
    }
}

// ---- FsBack 实现: tmpfs 查不到的文件兜底到这块真实磁盘 ----

fn fat_read_ent(e: usize, dst: &mut [u8]) -> usize {
    let mut sec = [0u8; SECTOR];
    unsafe { ata_read(DIR_LBA, sec.as_mut_ptr()) };
    let cluster = rd16(&sec, e + 26);
    let size = rd16(&sec, e + 28) | ((rd16(&sec, e + 30)) << 16);
    if size == 0 || cluster < 2 {
        return 0;
    }
    let mut data = [0u8; SECTOR];
    unsafe { ata_read(DATA_LBA + (cluster as u32 - 2), data.as_mut_ptr()) };
    let avail = if size > SECTOR { SECTOR } else { size };
    let want = if dst.len() < avail { dst.len() } else { avail };
    let src = data.as_ptr();
    let dd = dst.as_mut_ptr();
    unsafe {
        for k in 0..want {
            *dd.add(k) = *src.add(k);
        }
    }
    want
}

fn fat_write_ent(e: usize, src: &[u8]) -> usize {
    let mut sec = [0u8; SECTOR];
    unsafe { ata_read(DIR_LBA, sec.as_mut_ptr()) };
    let cluster = rd16(&sec, e + 26);
    if cluster < 2 {
        return 0;
    }
    let mut data = [0u8; SECTOR];
    unsafe { ata_read(DATA_LBA + (cluster as u32 - 2), data.as_mut_ptr()) };
    let n = if src.len() > SECTOR { SECTOR } else { src.len() };
    let dd = data.as_mut_ptr();
    let sp = src.as_ptr();
    unsafe {
        for k in 0..n {
            *dd.add(k) = *sp.add(k);
        }
    }
    unsafe { ata_write(DATA_LBA + (cluster as u32 - 2), data.as_ptr()) };
    // 目录项回写大小 (e+28..31, 裸指针避开越界检查符号)
    let size = n as u32;
    let sp2 = sec.as_mut_ptr();
    unsafe {
        *sp2.add(e + 28) = (size & 0xFF) as u8;
        *sp2.add(e + 29) = ((size >> 8) & 0xFF) as u8;
        *sp2.add(e + 30) = ((size >> 16) & 0xFF) as u8;
        *sp2.add(e + 31) = ((size >> 24) & 0xFF) as u8;
    }
    unsafe { ata_write(DIR_LBA, sec.as_ptr()) };
    n
}

fn fat_size_ent(e: usize) -> usize {
    let mut sec = [0u8; SECTOR];
    unsafe { ata_read(DIR_LBA, sec.as_mut_ptr()) };
    rd16(&sec, e + 28) | ((rd16(&sec, e + 30)) << 16)
}

fn fat_create(name: &[u8]) -> Option<usize> {
    let mut dir = [0u8; SECTOR];
    unsafe { ata_read(DIR_LBA, dir.as_mut_ptr()) };
    let e = free_entry(&dir)?;
    let dp = dir.as_mut_ptr();
    // 8+3 空格填充: 首段进名区 (≤8), 无扩展; 目录项字段全裸指针写
    unsafe {
        for d in 0..8 {
            let c = if d < name.len() && name[d] != b'.' { name[d] } else { b' ' };
            *dp.add(e + d) = c;
        }
        for d in 0..3 {
            *dp.add(e + 8 + d) = b' ';
        }
        *dp.add(e + 11) = 0x20; // 属性: 普通文件
        *dp.add(e + 26) = 0x02; // 首簇 2 (与 kernel.c fat12_create 一致)
        *dp.add(e + 27) = 0x00;
        *dp.add(e + 28) = 0;
        *dp.add(e + 29) = 0;
        *dp.add(e + 30) = 0;
        *dp.add(e + 31) = 0;
    }
    unsafe { ata_write(DIR_LBA, dir.as_ptr()) };
    Some(e)
}

/// 翻译器后端: tmpfs miss 时兜底的 fat12 磁盘
pub struct Fat12Back;

impl FsBack for Fat12Back {
    fn open(&mut self, name: &[u8]) -> Option<usize> {
        let mut dir = [0u8; SECTOR];
        unsafe { ata_read(DIR_LBA, dir.as_mut_ptr()) };
        match find_entry(&dir, name) {
            Some(e) => Some(e),
            None => fat_create(name).map(|e| e),
        }
    }
    fn read(&self, h: usize, _off: usize, dst: &mut [u8]) -> usize {
        fat_read_ent(h, dst)
    }
    fn write(&mut self, h: usize, _off: usize, src: &[u8]) -> usize {
        fat_write_ent(h, src)
    }
    fn size(&self, h: usize) -> usize {
        fat_size_ent(h)
    }
}

// ---- 宿主可测的纯函数 ----

#[cfg(test)]
mod tests {
    use super::*;

    fn mkentry(name8: &[u8]) -> [u8; SECTOR] {
        let mut dir = [0u8; SECTOR];
        // 项 0: 名 8 字节 + 扩展 3 空格 + 属性 + 簇 2 + 大小 8
        for d in 0..8 {
            dir[d] = if d < name8.len() { name8[d] } else { b' ' };
        }
        dir[11] = 0x20;
        dir[26] = 2;
        dir[28] = 8;
        // 项 1: 已删除标记
        dir[32] = 0xE5;
        // 项 2: 目录末尾
        dir[64] = 0x00;
        dir
    }

    #[test]
    fn find_by_name_and_skip_deleted() {
        let dir = mkentry(b"T");
        assert_eq!(find_entry(&dir, b"T"), Some(0));
        assert_eq!(find_entry(&dir, b"NOTEXIST"), None);
        // 空格填充的名字也匹配
        assert_eq!(find_entry(&dir, b"T.TXT"), Some(0));
    }

    #[test]
    fn name_fill_matches() {
        let dir = mkentry(b"WORLD");
        assert_eq!(find_entry(&dir, b"WORLD"), Some(0));
        assert_eq!(find_entry(&dir, b"W"), Some(0));
        assert_eq!(find_entry(&dir, b"WORLDD"), None);
    }

    #[test]
    fn free_entry_skips_used() {
        let mut dir = mkentry(b"T");
        assert_eq!(free_entry(&dir), Some(32)); // 项 0 占用 → 槽 1
        dir[32] = 0x00; // 项 1 变空
        assert_eq!(free_entry(&dir), Some(32));
    }

    #[test]
    fn stop_at_dir_end() {
        let dir = mkentry(b"T");
        assert_eq!(find_entry(&dir, b"ZZZZZZZZ"), None);
    }
}
