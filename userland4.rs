//! 第四个 Ring3 用户任务 —— VFS 翻译器客户端演示
//! 全程走 IPC: OPEN "OUT" → WRITE "VFS!" → READ 回来 → 把回读内容画上屏。
//! 回读字节等于写入字节 → 青色 "VFS 004 OK"; 不符 → 红色, 数字显示实际长度。
//! 文件本体在 tmpfs, 令牌在翻译器手里 —— 客户端只见了 fd 和字节。

use core::arch::asm;

const SYS_GLYPH: u64 = 2;
const SYS_IPC_SEND: u64 = 4;
const SYS_IPC_RECV: u64 = 5;
const SYS_FILL: u64 = 13;
const SYS_YIELD: u64 = 12;
const SYS_SLEEP: u64 = 14;

// 令牌静态走 crate 路径 (extern "C" 引不到 Rust 命名修饰符号)
fn token() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD4_TOKEN) as u64 }
}
fn ep_q() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD_EP_Q) as u64 }
}
fn ep_r() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD_EP_R) as u64 }
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

fn ipc_send(tok: u64, buf: *const u8, len: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_IPC_SEND, in("rdi") tok, in("rsi") buf, in("rdx") len,
            lateout("rax") ret,
        );
    }
    ret
}

fn ipc_recv(tok: u64, buf: *mut u8, len_out: *mut u64, cap_out: *mut u32) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_IPC_RECV, in("rdi") tok, in("rsi") buf,
            in("rdx") len_out, in("r8") cap_out,
            lateout("rax") ret,
        );
    }
    ret
}

fn yield_now() {
    unsafe {
        asm!("int 0x80", in("rax") SYS_YIELD, in("rdi") token(), lateout("rax") _,);
    }
}

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

static mut CLI_STACK: [u8; 16384] = [0; 16384];

pub fn stack_top() -> u64 {
    unsafe { (core::ptr::addr_of!(CLI_STACK) as u64) + 16384 }
}

static mut REQ: [u8; 44] = [0; 44];
static mut RESP: [u8; 48] = [0; 48];
static mut RBUF: [u8; 32] = [0; 32];

use crate::translator::{self, FS_OP_OPEN, FS_OP_READ, FS_OP_WRITE};

/// 走翻译器开文件, 返回 fd (-1 = 失败)
fn vfs_open(name: &[u8]) -> i32 {
    unsafe {
        let req = core::ptr::addr_of_mut!(REQ) as *mut u8;
        translator::build_request(
            FS_OP_OPEN, 0, 0, 0, name,
            &mut *(core::ptr::addr_of_mut!(REQ) as *mut [u8; translator::REQ_SIZE]),
        );
        let _ = ipc_send(ep_q(), req, translator::REQ_SIZE as u64);
        let rb = core::ptr::addr_of_mut!(RESP) as *mut u8;
        let mut mlen: u64 = 0;
        let mut mcap: u32 = 0;
        if ipc_recv(ep_r(), rb, core::ptr::addr_of_mut!(mlen), core::ptr::addr_of_mut!(mcap)) != 0 {
            return -1;
        }
        translator::resp_status(core::slice::from_raw_parts(rb, translator::RESP_SIZE))
    }
}

/// 走翻译器写文件
fn vfs_write(fd: i32, payload: &[u8]) -> u32 {
    unsafe {
        translator::build_request_data(
            FS_OP_WRITE, fd as u16, 0, payload.len() as u32, b"", payload,
            &mut *(core::ptr::addr_of_mut!(REQ) as *mut [u8; translator::REQ_DATA_SIZE]),
        );
        let rb = core::ptr::addr_of_mut!(REQ) as *mut u8;
        let _ = ipc_send(ep_q(), rb, translator::REQ_DATA_SIZE as u64);
        let pb = core::ptr::addr_of_mut!(RESP) as *mut u8;
        let mut mlen: u64 = 0;
        let mut mcap: u32 = 0;
        if ipc_recv(ep_r(), pb, core::ptr::addr_of_mut!(mlen), core::ptr::addr_of_mut!(mcap)) != 0 {
            return u32::MAX;
        }
        translator::resp_len(core::slice::from_raw_parts(pb, translator::RESP_SIZE))
    }
}

/// 走翻译器读文件, 字节落 RBUF, 返回长度
fn vfs_read(fd: i32) -> u32 {
    unsafe {
        translator::build_request(
            FS_OP_READ, fd as u16, 0, 32, b"",
            &mut *(core::ptr::addr_of_mut!(REQ) as *mut [u8; translator::REQ_SIZE]),
        );
        let rb = core::ptr::addr_of_mut!(REQ) as *mut u8;
        let _ = ipc_send(ep_q(), rb, translator::REQ_SIZE as u64);
        let pb = core::ptr::addr_of_mut!(RESP) as *mut u8;
        let mut mlen: u64 = 0;
        let mut mcap: u32 = 0;
        if ipc_recv(ep_r(), pb, core::ptr::addr_of_mut!(mlen), core::ptr::addr_of_mut!(mcap)) != 0 {
            return u32::MAX;
        }
        let resp = core::slice::from_raw_parts(pb, translator::RESP_SIZE);
        let n = translator::resp_len(resp) as usize;
        let dst = core::ptr::addr_of_mut!(RBUF) as *mut u8;
        let mut k = 0usize;
        let take = if n < 32 { n } else { 32 };
        while k < take {
            *dst.add(k) = translator::resp_data(resp, k);
            k += 1;
        }
        n as u32
    }
}

const CYAN: u32 = 0x00FFFF;
const RED: u32 = 0x0000FF;
const BLACK: u32 = 0x000000;

// 显示区: 行 12 (y=204), 先擦后画
const DX: i64 = 24;
const DY: i64 = 202;
const DW: i64 = 130;
const DH: i64 = 20;

// spawn 结果行 15 (y=255)
const SX: i64 = 24;
const SY: i64 = 253;

const SYS_SPAWN: u64 = 11;
const SYS_BRK: u64 = 15;

/// bump 堆: 请求 want 字节, 返回清零区指针 (0 = 失败)
fn sys_brk(want: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_BRK, in("rdi") token(), in("rsi") want,
            lateout("rax") ret,
        );
    }
    ret
}

/// 让内核把 tmpfs 里的 ELF 文件装载成新 ring3 任务, 参数压栈, 返回槽号
fn sys_spawn(name: *const u8, arg: *const u8, arglen: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_SPAWN, in("rdi") token(),
            in("rsi") name, in("rdx") arg, in("r8") arglen,
            lateout("rax") ret,
        );
    }
    ret
}

static mut NAME_BUF: [u8; 12] = [0; 12];
static mut ARG_BUF: [u8; 8] = [0; 8];
static mut SPAWNED: u8 = 0;

#[no_mangle]
pub extern "C" fn cli_start() -> ! {
    loop {
        unsafe {
            let fd = vfs_open(b"OUT");
            if fd >= 0 {
                let w = vfs_write(fd, b"VFS!");
                let n = vfs_read(fd);
                // 回读校验: "VFS!" 四字节原样回来 = 翻译器全链路通
                let rb = core::ptr::addr_of!(RBUF) as *const u8;
                let ok = w == 4
                    && n == 4
                    && *rb == b'V'
                    && *rb.add(1) == b'F'
                    && *rb.add(2) == b'S'
                    && *rb.add(3) == b'!';
                let color = if ok { CYAN } else { RED };
                fill(DX, DY, DW, DH, BLACK);
                glyph(b'V', DX + 3, DY + 2, color);
                glyph(b'F', DX + 15, DY + 2, color);
                glyph(b'S', DX + 27, DY + 2, color);
                glyph(b' ', DX + 39, DY + 2, color);
                let v = if ok { 4 } else { n.min(999) };
                glyph(b'0' + ((v / 100) % 10) as u8, DX + 51, DY + 2, color);
                glyph(b'0' + ((v / 10) % 10) as u8, DX + 63, DY + 2, color);
                glyph(b'0' + (v % 10) as u8, DX + 75, DY + 2, color);
                glyph(b' ', DX + 87, DY + 2, color);
                if ok {
                    glyph(b'O', DX + 99, DY + 2, color);
                    glyph(b'K', DX + 111, DY + 2, color);
                    // 翻译器通了之后 spawn 一次 hello, 参数 "SPAWN OK" 压栈
                    if SPAWNED == 0 {
                        SPAWNED = 1;
                        let nb = core::ptr::addr_of_mut!(NAME_BUF) as *mut u8;
                        let nm = b"hello";
                        let mut k = 0usize;
                        while k < nm.len() {
                            *nb.add(k) = nm[k];
                            k += 1;
                        }
                        let ab = core::ptr::addr_of_mut!(ARG_BUF) as *mut u8;
                        let am = b"SPAWN OK";
                        let mut k = 0usize;
                        while k < am.len() {
                            *ab.add(k) = am[k];
                            k += 1;
                        }
                        let slot = sys_spawn(nb, ab, am.len() as u64);
                        // 行15: 'S' + 三位 (成功=槽号青色, 失败=故障码红色)
                        fill(SX, SY, 70, 20, BLACK);
                        glyph(b'S', SX + 1, SY + 2, CYAN);
                        let (sv, col) = if slot == u64::MAX {
                            (core::ptr::read_volatile(&raw const crate::SPAWN_DBG) & 0xFFF, RED)
                        } else {
                            (slot.min(999) as u32, CYAN)
                        };
                        glyph(b'0' + ((sv / 100) % 10) as u8, SX + 13, SY + 2, col);
                        glyph(b'0' + ((sv / 10) % 10) as u8, SX + 25, SY + 2, col);
                        glyph(b'0' + (sv % 10) as u8, SX + 37, SY + 2, col);
                        // 堆演示: 分配 32B, 写 "HEAP" 读回核对, 行16 显 B+OK/NO
                        let hp = sys_brk(32);
                        if hp != 0 && hp != u64::MAX {
                            let p = hp as *mut u8;
                            *p = b'H';
                            *p.add(1) = b'E';
                            *p.add(2) = b'A';
                            *p.add(3) = b'P';
                            let okh = *p == b'H' && *p.add(1) == b'E' && *p.add(2) == b'A' && *p.add(3) == b'P';
                            fill(SX, SY + 22, 70, 20, BLACK);
                            glyph(b'B', SX + 1, SY + 24, if okh { CYAN } else { RED });
                            if okh {
                                glyph(b'O', SX + 13, SY + 24, CYAN);
                                glyph(b'K', SX + 25, SY + 24, CYAN);
                            } else {
                                glyph(b'N', SX + 13, SY + 24, RED);
                                glyph(b'3', SX + 25, SY + 24, RED);
                            }
                        }
                    }
                } else {
                    glyph(b'N', DX + 99, DY + 2, color);
                    glyph(b'3', DX + 111, DY + 2, color);
                }
            } else {
                fill(DX, DY, DW, DH, BLACK);
                glyph(b'N', DX + 3, DY + 2, RED);
                glyph(b'F', DX + 15, DY + 2, RED);
                glyph(b'S', DX + 27, DY + 2, RED);
            }
        }
        sleep(100); // 一秒一拍 VFS 往返, 不再空转
        yield_now();
    }
}
