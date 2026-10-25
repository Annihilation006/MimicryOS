//! 系统调用层（07-syscall.md）：分发 + 能力门控 + 帧缓冲渲染
//! 跳板在 usermode.asm（存 15 个寄存器 → rust_syscall_entry(帧) → 还原）
//! 帧内布局 = 跳板压栈顺序的逆序, 改跳板必须同步改这里

use crate::capability;

pub const SYS_PUTPIXEL: u64 = 1;
pub const SYS_GLYPH: u64 = 2;

pub const CAP_FRAMEBUF: u32 = capability::CAP_VMO; // 帧缓冲就是一种 VMO

/// 与跳板压栈布局一一对应（内存升序 = 压栈逆序）
#[repr(C)]
pub struct SyscallFrame {
    pub r15: u64, pub r14: u64, pub r13: u64, pub r12: u64,
    pub r11: u64, pub r10: u64, pub r9: u64, pub r8: u64,
    pub rbp: u64, pub rdi: u64, pub rsi: u64, pub rdx: u64,
    pub rcx: u64, pub rbx: u64, pub rax: u64,
}

/// 3x5 微点阵: (ascii, 5 行位图, bit0=最左)
pub const MICROFONT: &[(u8, [u8; 5])] = &[
    (b'R', [7, 5, 7, 6, 5]),
    (b'I', [7, 2, 2, 2, 7]),
    (b'N', [5, 7, 7, 5, 5]),
    (b'G', [7, 4, 5, 5, 7]),
    (b'3', [7, 1, 7, 1, 7]),
    (b' ', [0, 0, 0, 0, 0]),
    (b'O', [7, 5, 5, 5, 7]),
    (b'K', [5, 6, 4, 6, 5]),
];

/// 渲染后端: 内核侧写真帧, 宿主测试收进 Vec
pub trait FbSink {
    fn put(&mut self, x: i64, y: i64, color: u32);
    fn size(&self) -> (i64, i64);
}

pub struct Framebuffer {
    pub base: u64,
    pub scan: u64,
    pub w: i64,
    pub h: i64,
}

impl FbSink for Framebuffer {
    fn put(&mut self, x: i64, y: i64, color: u32) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        unsafe {
            let p = (self.base + (y as u64) * self.scan + (x as u64) * 4) as *mut u32;
            core::ptr::write_volatile(p, color);
        }
    }
    fn size(&self) -> (i64, i64) {
        (self.w, self.h)
    }
}

/// 帧内字形绘制: 3x5 位图放大 3 倍 → 9x15
pub fn draw_glyph(sink: &mut dyn FbSink, ch: u8, x: i64, y: i64, color: u32) -> u64 {
    let g = match MICROFONT.iter().find(|g| g.0 == ch) {
        Some(g) => g,
        None => return 1, // 字库里没有
    };
    for row in 0..5i64 {
        let bits = g.1[row as usize];
        for col in 0..3i64 {
            if bits & (1 << col) != 0 {
                for dy in 0..3i64 {
                    for dx in 0..3i64 {
                        sink.put(x + col * 3 + dx, y + row * 3 + dy, color);
                    }
                }
            }
        }
    }
    0
}

/// 分发核（纯逻辑可测）：
/// rax=号, rdi=能力令牌, rsi/rdx/r8=参数; 返回值写回帧 rax
/// cap_check(令牌) 由调用方接能力系统; 渲染走 sink
pub fn dispatch(
    f: &mut SyscallFrame,
    cap_check: fn(u32) -> bool,
    sink: &mut dyn FbSink,
) {
    let ret = match f.rax {
        SYS_PUTPIXEL => {
            if !cap_check(f.rdi as u32) {
                u64::MAX
            } else {
                sink.put(f.rsi as i64, f.rdx as i64, f.r8 as u32);
                0
            }
        }
        SYS_GLYPH => {
            if !cap_check(f.rdi as u32) {
                u64::MAX
            } else {
                draw_glyph(sink, f.rsi as u8, f.rdx as i64, f.r8 as i64, f.r9 as u32)
            }
        }
        _ => u64::MAX,
    };
    f.rax = ret;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemSink {
        px: Vec<(i64, i64, u32)>,
    }
    impl FbSink for MemSink {
        fn put(&mut self, x: i64, y: i64, color: u32) {
            self.px.push((x, y, color));
        }
        fn size(&self) -> (i64, i64) {
            (1024, 768)
        }
    }

    fn frame(nr: u64, cap: u64, a1: u64, a2: u64, a3: u64) -> SyscallFrame {
        SyscallFrame {
            r15: 0, r14: 0, r13: 0, r12: 0, r11: 0, r10: 0, r9: a3,
            r8: a3, rbp: 0, rdi: cap, rsi: a1, rdx: a2, rcx: 0, rbx: 0, rax: nr,
        }
    }

    #[test]
    fn t1_good_cap_draws() {
        let mut f = frame(SYS_PUTPIXEL, 1, 10, 20, 0xFF00FF);
        let mut s = MemSink { px: Vec::new() };
        dispatch(&mut f, |t| t == 1, &mut s);
        assert_eq!(f.rax, 0);
        assert_eq!(s.px, vec![(10, 20, 0xFF00FF)]);
    }

    #[test]
    fn t2_bad_cap_rejected() {
        let mut f = frame(SYS_PUTPIXEL, 99, 10, 20, 0xFFFFFF);
        let mut s = MemSink { px: Vec::new() };
        dispatch(&mut f, |t| t == 1, &mut s);
        assert_eq!(f.rax, u64::MAX);
        assert!(s.px.is_empty(), "没能力就不许画");
    }

    #[test]
    fn t3_glyph_pixels() {
        let mut f = frame(SYS_GLYPH, 1, b'O' as u64, 5, 0x00FFFF);
        let mut s = MemSink { px: Vec::new() };
        dispatch(&mut f, |t| t == 1, &mut s);
        assert_eq!(f.rax, 0);
        // O 的位图 [7,5,5,5,7] 放大 3 倍: 亮块 = 行0(3列) + 行1..3(2列) + 行4(3列)
        // 每实位块 3x3=9 像素: (3+2+2+2+3)块 * 9 = 108
        assert_eq!(s.px.len(), 108);
    }

    #[test]
    fn t4_unknown_glyph() {
        let mut f = frame(SYS_GLYPH, 1, b'Z' as u64, 5, 0xFFFFFF);
        let mut s = MemSink { px: Vec::new() };
        dispatch(&mut f, |t| t == 1, &mut s);
        assert_eq!(f.rax, 1);
        assert!(s.px.is_empty());
    }

    #[test]
    fn t5_unknown_syscall() {
        let mut f = frame(42, 1, 0, 0, 0);
        let mut s = MemSink { px: Vec::new() };
        dispatch(&mut f, |t| t == 1, &mut s);
        assert_eq!(f.rax, u64::MAX);
    }
}
