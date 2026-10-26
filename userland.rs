//! 用户态载荷（Ring 3）—— 纯 Rust
//! 被 spawn_user 丢进 ring3, 只靠 int 0x80 跟内核说话
//! 内联汇编仅限 int 指令本身, 逻辑全是 Rust

#![allow(dead_code)]

use core::arch::asm;

const SYS_PUTPIXEL: u64 = 1;
const SYS_GLYPH: u64 = 2;

// 真令牌由内核在发能力时写入 (capability 的槽位不从 1 起)
fn token() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD_TOKEN) as u64 }
}

fn token_ep() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD_EP) as u64 }
}

const SYS_IPC_SEND: u64 = 4;
const SYS_OPEN: u64 = 6;
const SYS_WRF: u64 = 7;
const SYS_VMO_CREATE: u64 = 9;
const SYS_YIELD: u64 = 12;
const SYS_SLEEP: u64 = 14;

/// 定时睡眠: 延迟 ticks (100Hz)
fn sleep(ticks: u64) {
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_SLEEP, in("rdi") token(),
            in("rsi") ticks,
            lateout("rax") _,
        );
    }
}

// 建一块共享内存, 返回基址 (第二个用户任务从那儿读)
fn vmo_create() -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_VMO_CREATE, in("rdi") 1,
            lateout("rax") ret,
        );
    }
    ret
}

// 主动让出 CPU
fn yield_now() {
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_YIELD, in("rdi") token(),
            lateout("rax") _,
        );
    }
}

// 往 IPC 通道丢一条消息
fn ipc_send(data: &[u8]) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_IPC_SEND, in("rdi") token_ep(),
            in("rsi") data.as_ptr(), in("rdx") data.len(),
            lateout("rax") ret,
        );
    }
    ret
}

// 开文件 → 拿到绑着该文件的能力令牌
fn sys_open(name: &[u8]) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_OPEN, in("rdi") token(),
            in("rsi") name.as_ptr(),
            lateout("rax") ret,
        );
    }
    ret
}

// 写文件
fn wrf(ftok: u64, data: &[u8]) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_WRF, in("rdi") ftok,
            in("rsi") data.as_ptr(), in("rdx") data.len(),
            lateout("rax") ret,
        );
    }
    ret
}

#[inline(always)]
fn putpixel(x: i64, y: i64, color: u32) -> i64 {
    let ret: i64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_PUTPIXEL, in("rdi") token(),
            in("rsi") x, in("rdx") y, in("r8") color as u64,
            lateout("rax") ret,
        );
    }
    ret
}

#[inline(always)]
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

const MSG: &[u8] = b"RING3 OK";
const PALETTE: [u32; 4] = [0x00FFFF, 0xFF00FF, 0xFFFFFF, 0x00FF00];

#[no_mangle]
pub extern "C" fn user_payload_start() -> ! {
    let ftok = sys_open(b"LOG"); // 文件能力令牌, 绑着 LOG
    let vbase = vmo_create(); // 共享内存: 计数器放这儿给任务B看
    let mut phase: usize = 0;
    loop {
        // 把相位写进文件, 内核 fs 监视线程读出来上屏
        let fill = (phase as u8) % 200 + 1;
        let buf = [fill; 16];
        let _ = wrf(ftok, &buf);
        // 共享内存计数器: 任务B 读到就会画出来
        if vbase != u64::MAX {
            unsafe {
                core::ptr::write_volatile(vbase as *mut u64, phase as u64);
            }
        }
        // 擦旧下划线
        let mut x = 27i64;
        while x < 100 {
            putpixel(x, 180, 0x000000);
            x += 1;
        }
        // 新下划线, 长度跟相位走
        let len = 8 + (phase as i64 % 60);
        x = 27;
        while x < 27 + len {
            putpixel(x, 180, 0xFFFFFF);
            x += 1;
        }
        // 文本
        let color = PALETTE[phase % 4];
        for (i, &ch) in MSG.iter().enumerate() {
            glyph(ch, 27 + (i as i64) * 12, 160, color);
        }
        phase += 1;
        // 跟内核里的 IPC 服务线程打个招呼: 发相位数过去
        let msg = (phase as u64).to_le_bytes();
        let _ = ipc_send(&msg);
        sleep(100); // 一秒一拍, 不再空转烧 CPU
    }
}
