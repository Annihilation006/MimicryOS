// async ipc + vmo. 控制消息走环形队列内联, 大数据以后挂 vmo
// 单核没锁, 多核再说 (TODO)

use crate::frame::FrameSource;

pub const MSG_PAYLOAD: usize = 48;
pub const NMSG: usize = 32;

#[derive(Clone, Copy)]
pub struct Message {
    pub cap: u32,
    pub len: u16,
    pub data: [u8; MSG_PAYLOAD],
}

#[derive(Debug, PartialEq, Eq)]
pub enum IpcErr {
    Full,
    Empty,
}

pub struct Channel {
    buf: *mut Message,
    cap: usize,
    head: usize,
    count: usize,
}

impl Channel {
    pub fn new(buf: *mut Message, n: usize) -> Channel {
        unsafe {
            for i in 0..n {
                *(buf.add(i)) = Message { cap: 0, len: 0, data: [0; MSG_PAYLOAD] };
            }
        }
        Channel { buf, cap: n, head: 0, count: 0 }
    }

    pub fn send(&mut self, m: &Message) -> Result<(), IpcErr> {
        if self.count == self.cap {
            return Err(IpcErr::Full);
        }
        // 别用 % : cap 是运行时值会生成除零 panic 桩, 裸机直链没这符号
        let mut tail = self.head + self.count;
        while tail >= self.cap {
            tail -= self.cap;
        }
        unsafe { *(self.buf.add(tail)) = *m; }
        self.count += 1;
        Ok(())
    }

    pub fn recv(&mut self, m: &mut Message) -> Result<(), IpcErr> {
        if self.count == 0 {
            return Err(IpcErr::Empty);
        }
        unsafe { *m = *(self.buf.add(self.head)); }
        self.head = if self.head + 1 >= self.cap { 0 } else { self.head + 1 };
        self.count -= 1;
        Ok(())
    }

    pub fn pending(&self) -> usize {
        self.count
    }
}

// ---- vmo ----

pub const VMO_MAX_PAGES: usize = 64;

#[derive(Clone, Copy)]
pub struct Vmo {
    pub frames: [u64; VMO_MAX_PAGES],
    pub npages: usize,
    pub refs: u32,
}

impl Vmo {
    pub fn create(fs: &mut dyn FrameSource, pages: usize) -> Option<Vmo> {
        if pages == 0 || pages > VMO_MAX_PAGES {
            return None;
        }
        let mut v = Vmo { frames: [0; VMO_MAX_PAGES], npages: pages, refs: 1 };
        for i in 0..pages {
            match fs.alloc_frame() {
                Some(p) => v.frames[i] = p,
                None => {
                    // 拿到几页就还几页, 别泄漏
                    for j in 0..i {
                        fs.free_frame(v.frames[j]);
                    }
                    return None;
                }
            }
        }
        Some(v)    }

    pub fn page(&self, i: usize) -> Option<u64> {
        if i < self.npages {
            Some(self.frames[i])
        } else {
            None
        }
    }

    pub fn pin(&mut self) {
        self.refs += 1;
    }

    // 引用归零归还帧, 返回是否真的释放了
    pub fn unpin(&mut self, fs: &mut dyn FrameSource) -> bool {
        if self.refs == 0 {
            return false;
        }
        self.refs -= 1;
        if self.refs == 0 {
            // 不用下标循环: no_std 直链没有 core 的 panic 桩
            for &f in self.frames.iter().take(self.npages) {
                fs.free_frame(f);
            }
            self.npages = 0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Arena {
        mem: Vec<u8>,
        next: usize,
        freed: Vec<u64>,
    }
    impl Arena {
        fn new(frames: usize) -> Arena {
            Arena { mem: vec![0u8; frames * 4096], next: 0, freed: vec![] }
        }
    }
    impl FrameSource for Arena {
        fn alloc_frame(&mut self) -> Option<u64> {
            // 释放过的帧优先复用, 对齐真实分配器语义
            if let Some(p) = self.freed.pop() {
                return Some(p);
            }
            if self.next + 4096 <= self.mem.len() {
                let o = self.next;
                self.next += 4096;
                Some(o as u64)
            } else {
                None
            }
        }
        fn free_frame(&mut self, phys: u64) {
            self.freed.push(phys);
        }
        fn frame_to_virt(&self, phys: u64) -> *mut u8 {
            unsafe { self.mem.as_ptr().add(phys as usize) as *mut u8 }
        }
    }

    fn mkmsg(tag: u8) -> Message {
        let mut m = Message { cap: 3, len: 4, data: [0; MSG_PAYLOAD] };
        m.data[0] = tag;
        m
    }

    #[test]
    fn fifo_order() {
        let mut storage = [Message { cap: 0, len: 0, data: [0; MSG_PAYLOAD] }; 8];
        let mut ch = Channel::new(storage.as_mut_ptr(), 8);
        assert!(ch.send(&mkmsg(1)).is_ok());
        assert!(ch.send(&mkmsg(2)).is_ok());
        let mut m = Message { cap: 0, len: 0, data: [0; MSG_PAYLOAD] };
        ch.recv(&mut m).unwrap();
        assert_eq!(m.data[0], 1);
        ch.recv(&mut m).unwrap();
        assert_eq!(m.data[0], 2);
        assert_eq!(ch.recv(&mut m), Err(IpcErr::Empty));
    }

    #[test]
    fn full_wraparound() {
        let mut storage = [Message { cap: 0, len: 0, data: [0; MSG_PAYLOAD] }; 4];
        let mut ch = Channel::new(storage.as_mut_ptr(), 4);
        for i in 0..4u8 {
            assert!(ch.send(&mkmsg(i)).is_ok());
        }
        assert_eq!(ch.send(&mkmsg(9)), Err(IpcErr::Full));
        let mut m = Message { cap: 0, len: 0, data: [0; MSG_PAYLOAD] };
        ch.recv(&mut m).unwrap();
        assert!(ch.send(&mkmsg(9)).is_ok()); // 环形复用
        ch.recv(&mut m).unwrap();
        assert_eq!(m.data[0], 1);
        assert_eq!(ch.pending(), 3);
    }

    #[test]
    fn vmo_create_pages_and_release() {
        let mut fs = Arena::new(8);
        let mut v = Vmo::create(&mut fs, 3).unwrap();
        assert_eq!(v.page(0), Some(0));
        assert_eq!(v.page(2), Some(2 * 4096));
        assert_eq!(v.page(3), None);

        v.pin();
        assert!(!v.unpin(&mut fs)); // 还有 1 个引用
        assert!(v.unpin(&mut fs)); // 归零, 帧还回去了
        assert_eq!(fs.freed.len(), 3);
    }

    #[test]
    fn vmo_alloc_failure_no_leak() {
        let mut fs = Arena::new(2);
        assert!(Vmo::create(&mut fs, 5).is_none()); // 只够 2 页
        // 全部还回来了: 再要 2 页应该成功
        let mut v2 = Vmo::create(&mut fs, 2).unwrap();
        assert_eq!(v2.npages, 2);
    }

    #[test]
    fn vmo_zero_or_too_big() {
        let mut fs = Arena::new(8);
        assert!(Vmo::create(&mut fs, 0).is_none());
        assert!(Vmo::create(&mut fs, VMO_MAX_PAGES + 1).is_none());
    }
}
