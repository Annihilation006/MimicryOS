//! 第五个 Ring3 用户任务 —— 键盘回显
//! SYS_READ(0) 从内核键盘队列取字符, 队列空内核挂起本任务, 有键唤醒。
//! 取到的字符用字形系统调用画上行 13, 逐格右移, 换行回卷。
//! shell 同样收到这些键 (它仍是键盘的唯一读者, 我们拿的是副本)。

use core::arch::asm;

fn token() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD4_TOKEN) as u64 }
}

const SYS_GLYPH: u64 = 2;
const SYS_READ: u64 = 0;
const SYS_FILL: u64 = 13;
const SYS_YIELD: u64 = 12;

/// 读一行里的一个字符; 队列空返回 0 (任务已被内核挂起唤醒过一轮)
fn read_ch(buf: *mut u8) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_READ, in("rdi") 0u64, in("rsi") buf, in("rdx") 1u64,
            lateout("rax") ret,
        );
    }
    ret
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

fn fill(x: i64, y: i64, w: i64, h: i64, color: u32) {
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_FILL, in("rdi") token(),
            in("rsi") x, in("rdx") y, in("r8") w, in("r9") h, in("r10") color as u64,
            lateout("rax") _,
        );
    }
}

fn yield_now() {
    unsafe {
        asm!("int 0x80", in("rax") SYS_YIELD, in("rdi") token(), lateout("rax") _,);
    }
}

const CYAN: u32 = 0x00FFFF;
const BLACK: u32 = 0x000000;

// 独立用户栈 (.bss 静态, 在被放行的 2MB 窗口内)
static mut ECHO_STACK: [u8; 4096] = [0; 4096];

pub fn stack_top() -> u64 {
    unsafe { (core::ptr::addr_of!(ECHO_STACK) as u64) + 4096 }
}

// 行 13 (y = 13*17 = 221), 列 3..15 循环
const EX0: i64 = 24;
const EY: i64 = 219;
const COLS: i64 = 12;

#[no_mangle]
pub extern "C" fn echo_start() -> ! {
    let mut col = 0i64;
    loop {
        let mut ch = [0u8; 1];
        let n = read_ch(ch.as_mut_ptr());
        if n == 1 {
            unsafe {
                if ch[0] == b'\n' {
                    col = 0;
                    // 整行擦一遍, 干净回卷
                    fill(EX0, EY, COLS * 12 + 6, 18, BLACK);
                } else {
                    // 先擦这一格再画: 换字符不留残影
                    let cx = EX0 + col * 12;
                    fill(cx, EY, 11, 18, BLACK);
                    glyph(ch[0], cx + 1, EY + 2, CYAN);
                    col = (col + 1) % COLS;
                }
            }
        }
        yield_now();
    }
}
