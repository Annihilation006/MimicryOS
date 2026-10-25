//! tmpfs（06 第一步）：内存文件存储, 纯逻辑, 宿主可测
//! 注意: 内核路径全走裸指针, 运行时下标会生成 bounds-check panic 桩链不上

pub const MAX_FILES: usize = 8;
pub const NAME_LEN: usize = 12;
pub const FILE_CAP: usize = 256;

#[derive(Clone, Copy)]
pub struct TmpFile {
    pub used: bool,
    pub name: [u8; NAME_LEN],
    pub size: usize,
    pub data: [u8; FILE_CAP],
}

pub struct TmpFs {
    pub f: [TmpFile; MAX_FILES],
}

impl TmpFs {
    pub const fn new() -> Self {
        TmpFs {
            f: [TmpFile {
                used: false,
                name: [0; NAME_LEN],
                size: 0,
                data: [0; FILE_CAP],
            }; MAX_FILES],
        }
    }

    fn ent(&self, h: usize) -> *mut TmpFile {
        (self.f.as_ptr() as *mut TmpFile).wrapping_add(h)
    }

    // 按名找文件, 没有就建; 返回文件下标 (满员 None)
    pub fn open(&mut self, name: &[u8]) -> Option<usize> {
        let mut key = [0u8; NAME_LEN];
        let n = if name.len() > NAME_LEN { NAME_LEN } else { name.len() };
        key[..n].copy_from_slice(&name[..n]);

        let base = self.f.as_ptr();
        for i in 0..MAX_FILES {
            let e = unsafe { &*base.add(i) };
            if e.used && e.name == key {
                return Some(i);
            }
        }
        for i in 0..MAX_FILES {
            let e = unsafe { &*base.add(i) };
            if !e.used {
                let m = unsafe { &mut *(base as *mut TmpFile).add(i) };
                m.used = true;
                m.name = key;
                m.size = 0;
                return Some(i);
            }
        }
        None
    }

    // 写文件, 超长截断; 返回实际写入字节数
    pub fn write(&mut self, h: usize, off: usize, src: &[u8]) -> usize {
        if h >= MAX_FILES || off >= FILE_CAP {
            return 0;
        }
        let e = unsafe { &mut *self.ent(h) };
        if !e.used {
            return 0;
        }
        let room = FILE_CAP - off;
        let n = if src.len() < room { src.len() } else { room };
        let mut k = 0;
        while k < n {
            e.data[off + k] = src[k];
            k += 1;
        }
        if off + n > e.size {
            e.size = off + n;
        }
        n
    }

    // 读文件, 返回实际读出字节数
    pub fn read(&self, h: usize, off: usize, dst: &mut [u8]) -> usize {
        if h >= MAX_FILES || off >= FILE_CAP {
            return 0;
        }
        let e = unsafe { &*self.ent(h) };
        if !e.used {
            return 0;
        }
        let avail = if e.size > off { e.size - off } else { 0 };
        let want = if dst.len() < avail { dst.len() } else { avail };
        let mut k = 0;
        while k < want {
            dst[k] = e.data[off + k];
            k += 1;
        }
        want
    }

    pub fn size(&self, h: usize) -> usize {
        if h >= MAX_FILES {
            return 0;
        }
        unsafe { (&*self.ent(h)).size }
    }

    pub fn del(&mut self, h: usize) {
        if h < MAX_FILES {
            unsafe { (&mut *self.ent(h)).used = false };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t1_open_create_reuse() {
        let mut fs = TmpFs::new();
        let a = fs.open(b"LOG").unwrap();
        let b = fs.open(b"LOG").unwrap();
        assert_eq!(a, b, "同名该拿到同一个");
        let c = fs.open(b"OTHER").unwrap();
        assert_ne!(a, c);
    }

    #[test]
    fn t2_write_read_roundtrip() {
        let mut fs = TmpFs::new();
        let h = fs.open(b"F").unwrap();
        assert_eq!(fs.write(h, 0, &[1, 2, 3, 4]), 4);
        let mut out = [0u8; 4];
        assert_eq!(fs.read(h, 0, &mut out), 4);
        assert_eq!(out, [1, 2, 3, 4]);
        assert_eq!(fs.size(h), 4);
    }

    #[test]
    fn t3_write_clamps() {
        let mut fs = TmpFs::new();
        let h = fs.open(b"BIG").unwrap();
        let big = [7u8; FILE_CAP + 50];
        let n = fs.write(h, 0, &big);
        assert_eq!(n, FILE_CAP);
        assert_eq!(fs.size(h), FILE_CAP);
    }

    #[test]
    fn t4_offset_write() {
        let mut fs = TmpFs::new();
        let h = fs.open(b"O").unwrap();
        fs.write(h, 0, &[9, 9]);
        fs.write(h, 1, &[1]);
        let mut out = [0u8; 2];
        fs.read(h, 0, &mut out);
        assert_eq!(out, [9, 1]);
    }

    #[test]
    fn t5_delete_and_bad_handle() {
        let mut fs = TmpFs::new();
        let h = fs.open(b"D").unwrap();
        fs.del(h);
        assert_eq!(fs.write(h, 0, &[1]), 0);
        assert_eq!(fs.read(h, 0, &mut [0u8; 4]), 0);
        assert_eq!(fs.write(999, 0, &[1]), 0);
        // 删了能重开
        let h2 = fs.open(b"D").unwrap();
        assert_eq!(h, h2);
    }

    #[test]
    fn t6_full_table() {
        let mut fs = TmpFs::new();
        for i in 0..MAX_FILES {
            let mut nm = [0u8; 4];
            nm[0] = b'A' + i as u8;
            assert!(fs.open(&nm).is_some());
        }
        assert_eq!(fs.open(b"ZZ"), None);
    }

    #[test]
    fn t7_name_truncated() {
        let mut fs = TmpFs::new();
        let h = fs.open(b"VERYLONGNAME12345").unwrap(); // 前 12 字节 = VERYLONGNAME
        let h2 = fs.open(b"VERYLONGNAME99999").unwrap(); // 同前 12 字节 → 同文件
        assert_eq!(h, h2);
        let h3 = fs.open(b"VERYLONGNAMX99999").unwrap(); // 第 12 字节 X≠E → 新文件
        assert_ne!(h, h3);
    }
}
