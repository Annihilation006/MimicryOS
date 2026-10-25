//! 第二个 Ring3 用户任务 —— 纯 Rust
//! 读任务A通过 VMO 共享内存发布的计数器, 用字形系统调用画上屏
//! 和任务A的通信走共享内存 (05 的零拷贝数据路), 画屏走能力门控系统调用

use core::arch::asm;

extern "C" {
    static vmo_table: u8; // vmo.asm 的表: 每槽 24B, [0]=基址
}

fn token() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD2_TOKEN) as u64 }
}

const SYS_GLYPH: u64 = 2;
const SYS_YIELD: u64 = 12;

fn yield_now() {
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_YIELD, in("rdi") token(),
            lateout("rax") _,
        );
    }
}

fn glyph(ch: u8, x: i64, y: i64, color: u32) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_GLYPH, in("rdi") token(),
            in("rsi") ch as u64, in("rdx") x, in("r8") y, in("r9") color as u64,
            lateout("rax") ret,
        );
    }
    ret
}

// 用户栈 (Rust 静态, 在被放行的 2MB 窗口内)
static mut USER2_STACK: [u8; 16384] = [0; 16384];

pub fn stack_top() -> u64 {
    unsafe { (core::ptr::addr_of!(USER2_STACK) as u64) + 16384 }
}

const CYAN: u32 = 0x00FFFF;

#[no_mangle]
pub extern "C" fn user2_start() -> ! {
    loop {
        unsafe {
            // 任务A 的 VMO 基址: vmo 表槽 0 (任务A create 后写入)
            let base = core::ptr::read_volatile(&raw const vmo_table as *const u64);
            if base != 0 {
                let v = core::ptr::read_volatile(base as *const u64);
                // 画 'V' + 三位十进制, 行 8 右侧 (x=110 起)
                glyph(b'V', 110, 136, CYAN);
                glyph(b':', 122, 136, CYAN);
                glyph(b'0' + ((v / 100) % 10) as u8, 134, 136, CYAN);
                glyph(b'0' + ((v / 10) % 10) as u8, 146, 136, CYAN);
                glyph(b'0' + (v % 10) as u8, 158, 136, CYAN);
            }
        }
        let mut spin = 1_000_000i64;
        while spin > 0 {
            core::hint::spin_loop();
            spin -= 1;
        }
        yield_now();
    }
}
