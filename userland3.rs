//! 第三个 Ring3 用户任务 —— VFS 翻译器服务 (01/06-vfs.md: 一切皆翻译器)
//! 阻塞在请求通道 (通道1) 上收 FsRequest, 经 translator::serve() 处理,
//! 背后用内核 tmpfs 系统调用干活, 应答发到应答通道 (通道2)。
//! 客户端只见不透明 fd, 文件令牌留在服务的 fd 表里不出门。

use core::arch::asm;

const SYS_IPC_SEND: u64 = 4;
const SYS_IPC_RECV: u64 = 5;
const SYS_OPEN: u64 = 6;
const SYS_WRF: u64 = 7;
const SYS_RDF: u64 = 8;
const SYS_YIELD: u64 = 12;

// 令牌静态走 crate 路径 (extern "C" 引不到 Rust 命名修饰符号)
fn t3() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD3_TOKEN) as u64 }
}
fn ep_q() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD_EP_Q) as u64 }
}
fn ep_r() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const crate::PAYLOAD_EP_R) as u64 }
}

fn sys_open(name: *const u8) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_OPEN, in("rdi") t3(), in("rsi") name,
            lateout("rax") ret,
        );
    }
    ret
}

fn wrf(tok: u32, buf: *const u8, len: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_WRF, in("rdi") tok as u64, in("rsi") buf, in("rdx") len,
            lateout("rax") ret,
        );
    }
    ret
}

fn rdf(tok: u32, buf: *mut u8, len: u64) -> u64 {
    let ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            in("rax") SYS_RDF, in("rdi") tok as u64, in("rsi") buf, in("rdx") len,
            lateout("rax") ret,
        );
    }
    ret
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

/// 阻塞收: 通道空则内核把本任务挂起, 返回消息长度
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
        asm!("int 0x80", in("rax") SYS_YIELD, in("rdi") t3(), lateout("rax") _,);
    }
}

// 用户栈 (Rust 静态, 在被放行的 2MB 窗口内)
static mut SRV_STACK: [u8; 16384] = [0; 16384];

pub fn stack_top() -> u64 {
    unsafe { (core::ptr::addr_of!(SRV_STACK) as u64) + 16384 }
}

// 收发缓冲与 fd 表 (ring3 可写的 .bss)
static mut REQ_BUF: [u8; 48] = [0; 48];
static mut RESP_BUF: [u8; 48] = [0; 48];
static mut NAME_BUF: [u8; 13] = [0; 13];
static mut IO_BUF: [u8; 32] = [0; 32];
static mut FD_TOK: [u32; 8] = [0; 8]; // 客户端 fd → 文件令牌, 0 = 空槽
static mut FD_NAME: [[u8; 12]; 8] = [[0; 12]; 8]; // 并行的名字绑定

use crate::translator::{self, FsBack};

/// 把内核 tmpfs 系统调用包成 FsBack: serve() 纯逻辑经它落到真文件系统
struct KernelFs;
impl FsBack for KernelFs {
    fn open(&mut self, name: &[u8]) -> Option<usize> {
        unsafe {
            // 同名复用: 每轮 RPC 都 sys_open 会漏一个能力槽, 几十轮就耗尽
            let tab = core::ptr::addr_of_mut!(FD_TOK) as *mut u32;
            let ntab = core::ptr::addr_of_mut!(FD_NAME) as *mut [u8; 12];
            let mut i = 0usize;
            while i < 8 {
                if *tab.add(i) != 0 {
                    let bound = &*ntab.add(i);
                    let mut same = true;
                    let mut k = 0usize;
                    while k < 12 {
                        let want = if k < name.len() { name[k] } else { 0 };
                        if bound[k] != want {
                            same = false;
                            break;
                        }
                        k += 1;
                    }
                    if same {
                        return Some(i);
                    }
                }
                i += 1;
            }
            let nb = core::ptr::addr_of_mut!(NAME_BUF) as *mut u8;
            let mut k = 0usize;
            while k < 12 {
                *nb.add(k) = if k < name.len() { name[k] } else { 0 };
                k += 1;
            }
            let tok = sys_open(nb) as u32;
            if tok == u32::MAX || tok == 0 {
                return None;
            }
            // 找空 fd 槽
            let mut i = 0usize;
            while i < 8 {
                if *tab.add(i) == 0 {
                    *tab.add(i) = tok;
                    let bound = &mut *ntab.add(i);
                    let mut k = 0usize;
                    while k < 12 {
                        *bound.as_mut_ptr().add(k) = if k < name.len() { name[k] } else { 0 };
                        k += 1;
                    }
                    return Some(i);
                }
                i += 1;
            }
            None
        }
    }
    fn read(&self, h: usize, _off: usize, dst: &mut [u8]) -> usize {
        let tok = unsafe { *(core::ptr::addr_of!(FD_TOK) as *const u32).add(h) };
        if tok == 0 {
            return 0;
        }
        let n = rdf(tok, dst.as_mut_ptr(), dst.len() as u64);
        n as usize
    }
    fn write(&mut self, h: usize, _off: usize, src: &[u8]) -> usize {
        let tok = unsafe { *(core::ptr::addr_of!(FD_TOK) as *const u32).add(h) };
        if tok == 0 {
            return 0;
        }
        let n = wrf(tok, src.as_ptr(), src.len() as u64);
        n as usize
    }
    fn size(&self, h: usize) -> usize {
        unsafe {
            let ob = core::ptr::addr_of_mut!(IO_BUF) as *mut u8;
            self.read(h, 0, core::slice::from_raw_parts_mut(ob, 32))
        }
    }
}

#[no_mangle]
pub extern "C" fn srv_start() -> ! {
    loop {
        unsafe {
            let rb = core::ptr::addr_of_mut!(REQ_BUF) as *mut u8;
            let pb = core::ptr::addr_of_mut!(RESP_BUF) as *mut u8;
            let mut mlen: u64 = 0;
            let mut mcap: u32 = 0;
            // 阻塞等请求: 通道空时内核挂起本任务
            let r = ipc_recv(ep_q(), rb, core::ptr::addr_of_mut!(mlen), core::ptr::addr_of_mut!(mcap));
            if r == 0 && mlen as usize >= translator::REQ_SIZE {
                let n = translator::serve(
                    core::slice::from_raw_parts(rb, mlen as usize),
                    &mut *(core::ptr::addr_of_mut!(RESP_BUF) as *mut [u8; translator::RESP_SIZE]),
                    &mut KernelFs,
                );
                // 应答回原请求通道对 (通道2), 带上请求方的能力标签
                let _ = ipc_send(ep_r(), pb, n as u64);
            }
        }
        yield_now();
    }
}
