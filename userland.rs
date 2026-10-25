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
    let mut phase: usize = 0;
    loop {
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
        let mut spin = 2_000_000i64;
        while spin > 0 {
            core::hint::spin_loop();
            spin -= 1;
        }
    }
}
