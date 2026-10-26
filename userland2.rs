//! 第二个 Ring3 用户任务 —— 纯 Rust
//! 读任务A通过 VMO 共享内存发布的计数器, 用字形系统调用画上屏
//! 和任务A的通信走共享内存 (05 的零拷贝数据路), 画屏走能力门控系统调用
//! 数字形状每轮都变, 不先擦会留残影: 滚动值先 fill 黑底再画

use core::arch::asm;

extern "C" {
    static vmo_table: u8; // vmo.asm 的表: 每槽 24B, [0]=基址
}

fn token() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD2_TOKEN) as u64 }
}

const SYS_GLYPH: u64 = 2;
const SYS_FILL: u64 = 13;
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

/// 矩形填充: rsi=x rdx=y r8=w r9=h r10=色
fn fill(x: i64, y: i64, w: i64, h: i64, color: u32) {
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_FILL, in("rdi") token(),
            in("rsi") x, in("rdx") y, in("r8") w, in("r9") h,
            in("r10") color as u64,
            lateout("rax") _,
        );
    }
}

// 用户栈 (Rust 静态, 在被放行的 2MB 窗口内)
static mut USER2_STACK: [u8; 16384] = [0; 16384];

pub fn stack_top() -> u64 {
    unsafe { (core::ptr::addr_of!(USER2_STACK) as u64) + 16384 }
}

const CYAN: u32 = 0x00FFFF;
const BLACK: u32 = 0x000000;

// 数字组包围盒: 三格 9x15, 前后各留 2px 余量
const VX: i64 = 132;
const NX: i64 = 192;
const DW: i64 = 36;
const DY: i64 = 134;
const DH: i64 = 18;

#[no_mangle]
pub extern "C" fn user2_start() -> ! {
    let mut n: u64 = 0;
    loop {
        unsafe {
            // 任务A 的 VMO 基址: vmo 表槽 0 (任务A create 后写入)
            let base = core::ptr::read_volatile(&raw const vmo_table as *const u64);
            if base != 0 {
                let v = core::ptr::read_volatile(base as *const u64);
                // 先擦后画: 数字形状轮换, 残影会让读数变成历次数字的并集
                fill(VX, DY, DW, DH, BLACK);
                glyph(b'V', 110, 136, CYAN);
                glyph(b'0' + ((v / 100) % 10) as u8, VX + 2, 136, CYAN);
                glyph(b'0' + ((v / 10) % 10) as u8, VX + 14, 136, CYAN);
                glyph(b'0' + (v % 10) as u8, VX + 26, 136, CYAN);
                // N = 自身循环计数 (调度活性的心跳); 连续读到 0 标红
                let stale = v == 0 && n > 3;
                let mark = if stale { 0xFF0000 } else { CYAN };
                fill(NX, DY, DW, DH, BLACK);
                glyph(b'N', 176, 136, mark);
                glyph(b'0' + ((n / 100) % 10) as u8, NX + 2, 136, mark);
                glyph(b'0' + ((n / 10) % 10) as u8, NX + 14, 136, mark);
                glyph(b'0' + (n % 10) as u8, NX + 26, 136, mark);
            }
        }
        n = n.wrapping_add(1);
        let mut spin = 1_000_000i64;
        while spin > 0 {
            core::hint::spin_loop();
            spin -= 1;
        }
        yield_now();
    }
}
