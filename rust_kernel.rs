// ============================================================
// MimicryOS test 分支 · Rust 内核侧编译单元
// 维护：homo-ai (Hugo_YuGuo)
//
// 模块装配：
//   frame      物理帧位图分配器（含 FrameSource 供给接口）
//   kmem       帧供给的内核小对象池（VMA 节点等自用）
//   vma        虚拟内存区域簿记（有序链表，演进目标 BTree）
//   pagetable  x86_64 四级页表 map/unmap/translate
//
// C 侧可直接调用的接口（声明见 TEST-BRANCH.md）：
//   rust_mem_init() / rust_frame_alloc() / rust_frame_free()
//   rust_mem_report() / rust_kmem_init(frames)
//   rust_map_page() / rust_unmap_page()
//   rust_vma_init() / rust_vma_mmap() / rust_vma_munmap()
//
// 本文件不接管入口、不改动任何既有 C 代码；
// 上述函数何时接线调用，等 main 作者点头（见 TEST-BRANCH.md）。
//
// 编译：rustc --target x86_64-unknown-none --emit=obj（见 Makefile）
// ============================================================

#![no_std]

#[path = "frame.rs"]
mod frame;
#[path = "kmem.rs"]
mod kmem;
#[path = "pagetable.rs"]
mod pagetable;
#[path = "vma.rs"]
mod vma;
#[path = "vmboot.rs"]
mod vmboot;
#[path = "aspace.rs"]
mod aspace;
#[path = "capability.rs"]
mod capability;
#[path = "ipc.rs"]
mod ipc;
#[path = "task.rs"]
mod task;
#[path = "syscall.rs"]
mod syscall;
#[path = "userland.rs"]
mod userland;
#[path = "userland2.rs"]
mod userland2;
#[path = "userland3.rs"]
mod userland3;
#[path = "userland4.rs"]
mod userland4;
#[path = "userland5.rs"]
mod userland5;
#[path = "tmpfs.rs"]
mod tmpfs;
#[path = "elf.rs"]
mod elf;
#[path = "translator.rs"]
mod translator;
#[path = "elfprog.rs"]
mod elfprog;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

// 裸机直链没人给 memmove（他的 C 侧只有 memset/memcpy），自己补
#[no_mangle]
pub unsafe extern "C" fn memmove(
    dst: *mut core::ffi::c_void,
    src: *const core::ffi::c_void,
    n: usize,
) -> *mut core::ffi::c_void {
    let d = dst as *mut u8;
    let s = src as *const u8;
    if (d as usize) < (s as usize) {
        let mut i = 0;
        while i < n {
            *d.add(i) = *s.add(i);
            i += 1;
        }
    } else {
        let mut i = n;
        while i > 0 {
            i -= 1;
            *d.add(i) = *s.add(i);
        }
    }
    dst
}

/* ---- main 分支既有 C 接口（kernel.c）---- */
extern "C" {
    fn fb_print(str: *const u8);
}

/* ---- cpu.asm 提供的汇编原语 ---- */
extern "C" {
    fn asm_read_cr0() -> u64;
    fn asm_read_cr3() -> u64;
    fn asm_write_cr3(val: u64);
    fn asm_cpuid(leaf: u32, subleaf: u32, out: *mut u32);
    fn asm_invlpg(addr: u64);
}

// ============================================================
// CPU 互操作桥（验证三方混合链接）
// ============================================================

#[no_mangle]
pub extern "C" fn rust_cpu_report() -> u64 {
    let cr0 = unsafe { asm_read_cr0() };
    let cr3 = unsafe { asm_read_cr3() };

    let mut cpu: [u32; 4] = [0; 4];
    unsafe { asm_cpuid(0, 0, cpu.as_mut_ptr()) };

    // CPUID(0) 厂商串：EBX/EDX/ECX → cpu[1]/cpu[3]/cpu[2]
    let mut vendor = [0u8; 13];
    vendor[0..4].copy_from_slice(&cpu[1].to_le_bytes());
    vendor[4..8].copy_from_slice(&cpu[3].to_le_bytes());
    vendor[8..12].copy_from_slice(&cpu[2].to_le_bytes());
    vendor[12] = b'\0';

    unsafe {
        fb_print(b"[rust] cpu bridge ok, vendor=\0".as_ptr());
        fb_print(vendor.as_ptr());
        fb_print(b"\n\0".as_ptr());
    }

    let _ = cr3;
    cr0
}

// ============================================================
// 物理帧管理（02-memory.md）
// ============================================================

const BOOTBOOT_INFO_ADDR: usize = 0xffff_ffff_ffe0_0000;

#[repr(C, packed)]
struct MMapEnt {
    ptr: u64,
    size: u64, // 低 4 位是类型，其余为字节数
}

#[repr(C, packed)]
struct Bootboot {
    magic: [u8; 4],
    size: u32,
    protocol: u8,
    fb_type: u8,
    numcores: u16,
    bspid: u16,
    timezone: i16,
    datetime: [u8; 8],
    initrd_ptr: u64,
    initrd_size: u64,
    fb_ptr: u64,
    fb_size: u32,
    fb_width: u32,
    fb_height: u32,
    fb_scanline: u32,
    _arch: [u8; 64],
    mmap: [MMapEnt; 1],
}

const MMAP_FREE: u64 = 1;

const MAX_FRAMES: usize = 1 << 20;
const BM_WORDS: usize = MAX_FRAMES / frame::WORD_BITS;

static mut FRAME_BITMAP: [u64; BM_WORDS] = [0; BM_WORDS];
static mut FRAME_ALLOC: Option<frame::FrameAllocator> = None;

/// 懒初始化: C 侧从不调 rust_mem_init, 第一次要帧时自己拉起分配器
unsafe fn ensure_frame_alloc() {
    let p: *mut Option<frame::FrameAllocator> = core::ptr::addr_of_mut!(FRAME_ALLOC);
    if (*p).is_none() {
        rust_mem_init();
    }
}

unsafe fn fa_opt() -> Option<&'static mut frame::FrameAllocator> {
    ensure_frame_alloc();
    let p: *mut Option<frame::FrameAllocator> = core::ptr::addr_of_mut!(FRAME_ALLOC);
    (*p).as_mut()
}

/// 解析 BOOTBOOT 内存图并初始化物理帧分配器。
/// 返回空闲帧总数；magic 校验失败返回 usize::MAX。
#[no_mangle]
pub extern "C" fn rust_mem_init() -> usize {
    unsafe {
        let bb = &*(BOOTBOOT_INFO_ADDR as *const Bootboot);
        let magic = bb.magic;
        if magic != *b"BOOT" {
            return usize::MAX;
        }

        let mut fa = frame::FrameAllocator::from_raw(
            core::ptr::addr_of_mut!(FRAME_BITMAP) as *mut u64,
            BM_WORDS,
            MAX_FRAMES,
        );
        fa.init_all_used();

        let hdr = 128usize;
        let ent = core::mem::size_of::<MMapEnt>();
        let nents = ((bb.size as usize).saturating_sub(hdr)) / ent;
        let first: *const MMapEnt = core::ptr::addr_of!(bb.mmap) as *const MMapEnt;

        for i in 0..nents {
            let e: MMapEnt = core::ptr::read(first.add(i));
            let typ = e.size & 0xF;
            let bytes = e.size & !0xFu64;
            if typ != MMAP_FREE || bytes == 0 {
                continue;
            }
            let start = e.ptr;
            let end = e.ptr.saturating_add(bytes);
            let f0 = ((start + frame::FRAME_SIZE - 1) / frame::FRAME_SIZE) as usize;
            let f1 = ((end + frame::FRAME_SIZE - 1) / frame::FRAME_SIZE) as usize;
            // 6MB 以下: 恒等映射盲区 + 内核页表 + 直映射表帧 + SPAWN/BRK
            // 恒等窗口的专属地盘, 分配器从 40MB 起发帧, 井水不犯河水
            let f0 = f0.max(10240);
            if f1 > f0 {
                fa.mark_free_range(f0, f1 - f0);
            }
        }

        let free = fa.free_count();
        let p: *mut Option<frame::FrameAllocator> = core::ptr::addr_of_mut!(FRAME_ALLOC);
        *p = Some(fa);
        free
    }
}

#[no_mangle]
pub extern "C" fn rust_frame_alloc() -> u64 {
    match unsafe { fa_opt() } {
        Some(fa) => match unsafe { fa.alloc() } {
            Some(idx) => idx as u64,
            None => u64::MAX,
        },
        None => u64::MAX,
    }
}

#[no_mangle]
pub extern "C" fn rust_frame_free(frame_no: u64) -> i32 {
    match unsafe { fa_opt() } {
        Some(fa) => {
            if unsafe { fa.free(frame_no as usize) } {
                0
            } else {
                -1
            }
        }
        None => -1,
    }
}

// ============================================================
// 内核小对象池 + VMA 簿记
// ============================================================

static mut KMEM: kmem::Kmem = kmem::Kmem::new();

/// 内核 VMA 竞技场：0xFFFF_8800_0000_0000 起 8TiB，
/// 避开 BOOTBOOT 保留区（fb / mmio / bootboot 结构 / 内核本体）。
const KERN_VMA_START: u64 = 0xFFFF_8800_0000_0000;
const KERN_VMA_END: u64 = 0xFFFF_9000_0000_0000;

static mut VMA_NODES: [vma::VmaNode; 256] = [vma::VmaNode::EMPTY; 256];
static mut VMA: Option<vma::VmaManager> = None;

unsafe fn vma_opt() -> Option<&'static mut vma::VmaManager> {
    let p: *mut Option<vma::VmaManager> = core::ptr::addr_of_mut!(VMA);
    (*p).as_mut()
}

/// 从物理帧分配器取 n 帧供给小对象池，返回新增字节数。
#[no_mangle]
pub extern "C" fn rust_kmem_init(frames: u64) -> usize {
    unsafe {
        let k: *mut kmem::Kmem = core::ptr::addr_of_mut!(KMEM);
        match fa_opt() {
            Some(fa) => (*k).add_frames(fa, frames as usize),
            None => 0,
        }
    }
}

/// 初始化内核 VMA 管理器（节点池 256 个，.bss 静态供给）。
#[no_mangle]
pub extern "C" fn rust_vma_init() -> i32 {
    unsafe {
        let pool = core::ptr::addr_of_mut!(VMA_NODES) as *mut vma::VmaNode;
        let p: *mut Option<vma::VmaManager> = core::ptr::addr_of_mut!(VMA);
        *p = Some(vma::VmaManager::new(pool, 256, KERN_VMA_START, KERN_VMA_END));
        0
    }
}

/// 在内核 VMA 竞技场切一块（len 字节，页对齐化），返回起始虚拟地址。
#[no_mangle]
pub extern "C" fn rust_vma_mmap(len: u64, flags: u64) -> u64 {
    match unsafe { vma_opt() } {
        Some(m) => m.mmap(len, flags).unwrap_or(u64::MAX),
        None => u64::MAX,
    }
}

/// 取消一块 VMA 簿记；返回 0=有变化，-1=无变化/未初始化。
#[no_mangle]
pub extern "C" fn rust_vma_munmap(start: u64, len: u64) -> i32 {
    match unsafe { vma_opt() } {
        Some(m) => {
            if m.munmap(start, len) {
                0
            } else {
                -1
            }
        }
        None => -1,
    }
}

// ============================================================
// 页表操作（作用于当前 CR3 指向的地址空间）
// ============================================================

/// flags 位定义见 pagetable.rs（PTE_WRITABLE=1<<1, PTE_USER=1<<2,
/// PTE_NO_EXEC=1<<63），C 侧传原始位组合即可。
#[no_mangle]
pub extern "C" fn rust_map_page(vaddr: u64, paddr: u64, flags: u64) -> i32 {
    unsafe {
        let pml4 = asm_read_cr3() & pagetable::PHYS_MASK;
        let fa = match fa_opt() {
            Some(f) => f,
            None => return -1,
        };
        match pagetable::map_page(fa, pml4, vaddr, paddr, flags) {
            Ok(()) => {
                asm_invlpg(vaddr);
                0
            }
            Err(_) => -1,
        }
    }
}

#[no_mangle]
pub extern "C" fn rust_unmap_page(vaddr: u64) -> i32 {
    unsafe {
        let pml4 = asm_read_cr3() & pagetable::PHYS_MASK;
        let fa = match fa_opt() {
            Some(f) => f,
            None => return -1,
        };
        match pagetable::unmap_page(fa, pml4, vaddr) {
            Ok(()) => {
                asm_invlpg(vaddr);
                0
            }
            Err(_) => -1,
        }
    }
}

// ============================================================
// 能力 + IPC
// ============================================================

const NCAPS: usize = 256;
const NCHMSG: usize = 32;
const NCHAN: usize = 3; // 0=ipc服务 1=VFS翻译器请求 2=VFS翻译器应答
const NVMO: usize = 8;

static mut CAP_NODES: [capability::CapNode; NCAPS] =
    [capability::CapNode { obj_type: 0, obj_id: 0, parent: 0, first_child: capability::NO_SLOT, next_sibling: capability::NO_SLOT }; NCAPS];
static mut CAP_USED: [bool; NCAPS] = [false; NCAPS];
static mut CAP_FREE: [u32; NCAPS] = [0; NCAPS];
static mut CAPS: Option<capability::CSpace> = None;

static mut CH_STORAGE: [[ipc::Message; NCHMSG]; NCHAN] =
    [[ipc::Message { cap: 0, len: 0, data: [0; ipc::MSG_PAYLOAD] }; NCHMSG]; NCHAN];
static mut CHAN: [Option<ipc::Channel>; NCHAN] = [None, None, None];

static mut VMO_TABLE: [ipc::Vmo; NVMO] =
    [ipc::Vmo { frames: [0; ipc::VMO_MAX_PAGES], npages: 0, refs: 0 }; NVMO];

unsafe fn caps_opt() -> Option<&'static mut capability::CSpace> {
    let p: *mut Option<capability::CSpace> = core::ptr::addr_of_mut!(CAPS);
    (*p).as_mut()
}

#[no_mangle]
pub extern "C" fn rust_cap_init() -> i32 {
    unsafe {
        let p: *mut Option<capability::CSpace> = core::ptr::addr_of_mut!(CAPS);
        *p = Some(capability::CSpace::new(
            core::ptr::addr_of_mut!(CAP_NODES) as *mut capability::CapNode,
            core::ptr::addr_of_mut!(CAP_USED) as *mut bool,
            core::ptr::addr_of_mut!(CAP_FREE) as *mut u32,
            NCAPS,
        ));
    }
    0
}

#[no_mangle]
pub extern "C" fn rust_cap_grant(obj_type: u32, obj_id: u32) -> u32 {
    match unsafe { caps_opt() } {
        Some(c) => c.grant(obj_type, obj_id, capability::NO_SLOT),
        None => capability::NO_SLOT,
    }
}

#[no_mangle]
pub extern "C" fn rust_cap_derive(parent: u32) -> u32 {
    match unsafe { caps_opt() } {
        Some(c) => c.derive(parent),
        None => capability::NO_SLOT,
    }
}

#[no_mangle]
pub extern "C" fn rust_cap_revoke(slot: u32) -> usize {
    match unsafe { caps_opt() } {
        Some(c) => c.revoke(slot),
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn rust_cap_lookup(slot: u32, out_type: *mut u32, out_id: *mut u32) -> i32 {
    match unsafe { caps_opt() } {
        Some(c) => match c.lookup(slot) {
            Some((t, id)) => {
                unsafe {
                    *out_type = t;
                    *out_id = id;
                }
                0
            }
            None => -1,
        },
        None => -2,
    }
}

#[no_mangle]
pub extern "C" fn rust_ipc_channel_init() -> i32 {
    unsafe {
        let base = core::ptr::addr_of_mut!(CHAN) as *mut Option<ipc::Channel>;
        let stor = core::ptr::addr_of_mut!(CH_STORAGE) as *mut ipc::Message;
        let mut i = 0usize;
        while i < NCHAN {
            *(base.add(i)) = Some(ipc::Channel::new(stor.add(i * NCHMSG), NCHMSG));
            i += 1;
        }
    }
    0
}

/// 端点令牌 → (通道号, 通道). 令牌必须是 CAP_ENDPOINT 且 id 落在通道表内
unsafe fn ep_channel(token: u32) -> Option<(usize, *mut ipc::Channel)> {
    let c = caps_opt()?;
    match (*c).lookup(token) {
        Some((t, id)) if t == capability::CAP_ENDPOINT && (id as usize) < NCHAN => {
            let base = core::ptr::addr_of_mut!(CHAN) as *mut Option<ipc::Channel>;
            match (*base.add(id as usize)).as_mut() {
                Some(ch) => Some((id as usize, ch as *mut ipc::Channel)),
                None => None,
            }
        }
        _ => None,
    }
}

/// 通道 c 上有新消息就把登记的等待者叫醒 (发送侧统一负责)
unsafe fn chan_wake(c: usize) {
    let w = core::ptr::addr_of_mut!(CHAN_WAITER) as *mut usize;
    let idx = *w.add(c);
    if idx != usize::MAX {
        let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
        (*s).wake(idx);
        *w.add(c) = usize::MAX;
    }
}

#[no_mangle]
pub extern "C" fn rust_ipc_send(cap: u32, buf: *const u8, len: u64) -> i32 {
    let len = len as usize;
    if len > ipc::MSG_PAYLOAD {
        return -2;
    }
    let (cid, chp) = match unsafe { ep_channel(cap) } {
        Some(x) => x,
        None => return -3,
    };
    let ch = unsafe { &mut *chp };
    let mut m = ipc::Message { cap, len: len as u16, data: [0; ipc::MSG_PAYLOAD] };
    unsafe {
        let dst = m.data.as_mut_ptr();
        for i in 0..len {
            *dst.add(i) = *buf.add(i);
        }
    }
    match ch.send(&m) {
        Ok(()) => {
            unsafe { chan_wake(cid) };
            0
        }
        Err(_) => -1,
    }
}

/// 内核线程直接往指定通道投递 (翻译器应答用), 唤醒逻辑同上
unsafe fn chan_send_to(cid: usize, buf: &[u8], cap: u32) -> i32 {
    if cid >= NCHAN || buf.len() > ipc::MSG_PAYLOAD {
        return -2;
    }
    let base = core::ptr::addr_of_mut!(CHAN) as *mut Option<ipc::Channel>;
    let ch = match (*base.add(cid)).as_mut() {
        Some(c) => c,
        None => return -3,
    };
    let mut m = ipc::Message { cap, len: buf.len() as u16, data: [0; ipc::MSG_PAYLOAD] };
    unsafe {
        let dst = m.data.as_mut_ptr();
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, buf.len());
    }
    match ch.send(&m) {
        Ok(()) => {
            chan_wake(cid);
            0
        }
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn rust_ipc_recv(buf: *mut u8, len_out: *mut u64, cap_out: *mut u32) -> i32 {
    unsafe { rust_ipc_recv_ch(0, buf, len_out, cap_out) }
}

#[no_mangle]
pub extern "C" fn rust_ipc_recv_ch(cid: usize, buf: *mut u8, len_out: *mut u64, cap_out: *mut u32) -> i32 {
    if cid >= NCHAN {
        return -3;
    }
    let base = unsafe { core::ptr::addr_of_mut!(CHAN) as *mut Option<ipc::Channel> };
    let ch = match unsafe { (*base.add(cid)).as_mut() } {
        Some(c) => c,
        None => return -3,
    };
    let mut m = ipc::Message { cap: 0, len: 0, data: [0; ipc::MSG_PAYLOAD] };
    match ch.recv(&mut m) {
        Ok(()) => {
            // len 钳到载荷上限, 用裸指针拷: 下标索引会生成 bounds-check panic 桩
            let n = (m.len as usize).min(ipc::MSG_PAYLOAD);
            let src = m.data.as_ptr();
            for i in 0..n {
                unsafe { *buf.add(i) = *src.add(i); }
            }
            unsafe {
                *len_out = m.len as u64;
                *cap_out = m.cap;
            }
            0
        }
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn rust_vmo_create(pages: u64) -> i32 {
    unsafe {
        let fa = match fa_opt() {
            Some(f) => f,
            None => return -3,
        };
        let table = core::ptr::addr_of_mut!(VMO_TABLE) as *mut ipc::Vmo;
        for i in 0..NVMO {
            let slot = &mut *table.add(i);
            if slot.refs == 0 && slot.npages == 0 {
                match ipc::Vmo::create(fa, pages as usize) {
                    Some(v) => {
                        *slot = v;
                        return i as i32;
                    }
                    None => return -1,
                }
            }
        }
        -2 // 表满
    }
}

#[no_mangle]
pub extern "C" fn rust_vmo_unref(handle: i32) -> i32 {
    if handle < 0 || handle as usize >= NVMO {
        return -1;
    }
    unsafe {
        let fa = match fa_opt() {
            Some(f) => f,
            None => return -3,
        };
        let slot = &mut *(core::ptr::addr_of_mut!(VMO_TABLE) as *mut ipc::Vmo).add(handle as usize);
        if slot.npages == 0 {
            return -2;
        }
        let _ = slot.unpin(fa);
        0
    }
}

// ============================================================
// 抢占式调度（03-process 第一步）+ 两个 Rust 内核线程
// ============================================================

static mut SCHED: task::Scheduler = task::Scheduler::new();
static mut STACK_A: [u8; 16384] = [0; 16384];
static mut STACK_B: [u8; 16384] = [0; 16384];
static mut CNT_A: u32 = 0;
static mut CNT_B: u32 = 0;
pub static mut PAYLOAD_TOKEN: u32 = 1; // rust_tasks_init 里发真值
pub static mut PAYLOAD_EP: u32 = 1;    // 端点能力: IPC 通道的钥匙
pub static mut PAYLOAD2_TOKEN: u32 = 1; // 第二个用户任务的画屏钥匙
pub static mut PAYLOAD3_TOKEN: u32 = 1; // 翻译器服务的过门钥匙
pub static mut PAYLOAD4_TOKEN: u32 = 1; // VFS 客户端的画屏钥匙
pub static mut PAYLOAD_EP_Q: u32 = 1;  // 通道1 (翻译器请求) 端点
pub static mut PAYLOAD_EP_R: u32 = 1;  // 通道2 (翻译器应答) 端点

extern "C" {
    fn fb_putchar(cx: i32, cy: i32, c: u8, color: u32);
    fn fb_putpixel(x: i32, y: i32, color: u32);
    fn vmo_init(); // vmo.asm: 共享内存池
    static user_stack_top: u8; // usermode.asm 的符号
}

// Rust 侧自己的 tick 计数 (rust_tick_hook 每 tick +1), 睡眠时限的基准
static mut RUST_TICKS: u64 = 0;

fn tick_now() -> u64 {
    unsafe { core::ptr::read_volatile(&raw const RUST_TICKS) }
}

// 端点能力检查: IPC 系统调用的门票
fn ep_cap_check(token: u32) -> bool {
    unsafe {
        let c: *mut Option<capability::CSpace> = core::ptr::addr_of_mut!(CAPS);
        match (*c).as_mut() {
            Some(cs) => matches!(cs.lookup(token), Some((t, _)) if t == capability::CAP_ENDPOINT),
            None => false,
        }
    }
}

// ---- 帧缓冲参数 (rust_tasks_init 从 bootboot 抄) ----
static mut FB_BASE: u64 = 0;
static mut FB_SCAN: u64 = 4096;
static mut FB_W: i64 = 1024;
static mut FB_H: i64 = 768;

// Ring3 放行: 走直映射改页表 (1MB 以下没恒等映射, 老 asm 版一直在对空气写)
// 沿 PML4→PDPT→PD 全路 U/S+清NX, 末级 4K 时 512 个 PTE 全放行
unsafe fn patch_user_region(va: u64) {
    let mask = 0x000F_FFFF_FFFF_F000;
    let mut table = asm_read_cr3() & mask;

    let i4 = ((va >> 39) & 511) as usize;
    let sl = (table + (i4 as u64) * 8) as *mut u64;
    let mut e = *sl;
    e |= 6;
    e &= !0x8000_0000_0000_0000;
    *sl = e;
    table = e & mask;

    let i3 = ((va >> 30) & 511) as usize;
    let sl = (table + (i3 as u64) * 8) as *mut u64;
    e = *sl;
    let big1 = e & 0x80 != 0;
    e |= 6;
    e &= !0x8000_0000_0000_0000;
    *sl = e;
    if big1 {
        tlb_flush();
        return;
    }
    table = e & mask;

    let i2 = ((va >> 21) & 511) as usize;
    let sl = (table + (i2 as u64) * 8) as *mut u64;
    e = *sl;
    let big2 = e & 0x80 != 0;
    e |= 6;
    e &= !0x8000_0000_0000_0000;
    *sl = e;
    if big2 {
        tlb_flush();
        return;
    }
    table = e & mask;

    for i in 0..512u64 {
        let sl = (table + i * 8) as *mut u64;
        let pte = *sl;
        if pte & 1 != 0 {
            *sl = (pte | 6) & !0x8000_0000_0000_0000;
        }
    }
    tlb_flush();
}

unsafe fn tlb_flush() {
    let c = asm_read_cr3();
    asm_write_cr3(c);
}

// tmpfs 实例: 载荷 open/write, 监视线程 read
static mut FS: tmpfs::TmpFs = tmpfs::TmpFs::new();

// SYS_SPAWN 的装载窗口: 每个动态进程 2 页 (段 + 栈), 基址递增
// BOOTBOOT 把 RAM 恒等映射成 2MB 大页, 4K map_page 插不进去 ——
// 与现有用户区同款打法: 恒等物理页上 patch_user_region 放行, 直映射拷段
static mut SPAWN_DBG: u32 = 0; // 故障码: 6=解析 7=拷贝 8=槽满 0xFF=成功
const SPAWN_VBASE: u64 = 0x0060_0000; // 6MB, 在帧分配器保留线以上
// 装载窗口表: 窗口随任务槽绑定, 任务退出即回收复用 (NW 个 = 调度器容量)
const NW: usize = 12;
static mut WINTAB: [usize; NW] = [usize::MAX; NW]; // 窗口 → 任务槽, MAX = 空闲

// 用户态 bump 堆: 16MB 起 4MB 上限, SYS_BRK 按需切清零块
static mut USER_BRK: u64 = 0x0100_0000;
const USER_BRK_END: u64 = 0x0140_0000;

/// 把 tmpfs 里的 ELF 镜像装载成 ring3 任务。arg 为启动参数 (≤64B)。
/// 返回任务槽号, 失败 u64::MAX。
unsafe fn spawn_elf_image(bin: &[u8], arg: &[u8]) -> u64 {
    let dbg = core::ptr::addr_of_mut!(SPAWN_DBG);
    *dbg = 0;
    // 领一个空闲装载窗口 (与任务槽绑定, 退出时归还)
    let mut w = NW;
    let wt = core::ptr::addr_of_mut!(WINTAB) as *mut usize;
    let mut k = 0usize;
    while k < NW {
        if *wt.add(k) == usize::MAX {
            w = k;
            break;
        }
        k += 1;
    }
    if w == NW {
        *dbg = 9; // 窗口耗尽
        return u64::MAX;
    }
    let base = SPAWN_VBASE + (w as u64) * 0x2000;
    // 两页放行为用户可写可执行
    patch_user_region(base);
    patch_user_region(base + 0x1000);
    // 段按差值平移: 镜像里的 p_vaddr 与运行基址可以不同 (代码 RIP 相对寻址)
    // 直映射没铺之前走恒等 (低 RAM phys==virt), 铺了之后走直映射窗口
    let seg_virt = if frame::DIRECT {
        (base + frame::DIRECT_BASE) as *mut u8
    } else {
        base as *mut u8
    };
    let (img_vaddr, entry0) = match crate::elf::ElfFile::parse(bin) {
        Ok(e) => {
            *dbg = 0x60 | (e.seg_count as u32);
            let mut iv = 0u64;
            if e.seg_count > 0 {
                if let Some(s0) = e.segments[0] {
                    iv = s0.vaddr;
                }
            }
            (iv, e.entry)
        }
        Err(_) => {
            *dbg = 6;
            return u64::MAX;
        }
    };
    let loaded = elfprog::load(bin, |va, bytes, _| {
        if va < img_vaddr {
            return Err(crate::elf::ElfError::TruncatedProgramHeader);
        }
        let off = (va - img_vaddr) as usize;
        if off + bytes.len() > 4096 {
            return Err(crate::elf::ElfError::TruncatedProgramHeader);
        }
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), seg_virt.add(off), bytes.len());
        Ok(())
    });
    if loaded.is_err() {
        *dbg = 7;
        return u64::MAX;
    }
    // 参数块放栈页底部固定地址 (约定: base+0x1000 = len, base+0x1008 = ptr,
    // 程序用 RIP 相对读), 参数串紧随其后; 与栈顶活动区相距整页, 无碰撞
    let mut alen = arg.len();
    if alen > 64 {
        alen = 64;
    }
    if alen > 0 {
        let stkw = if frame::DIRECT {
            (base + 0x1000 + frame::DIRECT_BASE) as *mut u8
        } else {
            (base + 0x1000) as *mut u8
        };
        core::ptr::write_volatile(stkw as *mut u64, alen as u64);
        core::ptr::write_volatile(stkw.wrapping_add(8) as *mut u64, base + 0x1010);
        let argv = stkw.wrapping_add(16);
        core::ptr::copy_nonoverlapping(arg.as_ptr(), argv, alen);
    }
    *dbg = 0xFF;
    // 入口平移: 镜像内地址 → 运行基址; 栈顶 = 第二页顶端
    let entry = entry0 - img_vaddr + base;
    let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
    match (*s).spawn_user(entry, base + 0x2000) {
        Some(i) => {
            // 窗口与任务槽绑定, 退出时按槽归还
            *wt.add(w) = i;
            i as u64
        }
        None => {
            *dbg = 8;
            u64::MAX
        }
    }
}

// ---- 键盘输入队列: shell 的 keyboard_read 钩子喂入, SYS_READ 取走 ----
// shell 是键盘的唯一读者(别动他的活), 我们只拿副本; 可打印字符 + 换行入队
static mut KB_BUF: [u8; 32] = [0; 32];
static mut KB_HEAD: usize = 0; // 写位
static mut KB_TAIL: usize = 0; // 读位
static mut KB_WAITER: usize = usize::MAX; // 阻塞在 read 上的任务

/// shell 每收到一个字符喂一份副本; 小写转大写 (字库只有大写), 只收可打印 ASCII 和换行
#[no_mangle]
pub extern "C" fn rust_kb_feed(c: u8) {
    let up = if c >= b'a' && c <= b'z' { c - 32 } else { c };
    let printable = up >= 0x20 && up < 0x7F;
    if !printable && up != b'\n' {
        return;
    }
    unsafe {
        let head = KB_HEAD;
        let next = (head + 1) % 32;
        if next == KB_TAIL {
            return; // 满: 丢新字节, 键盘流丢一个字符无害
        }
        *(core::ptr::addr_of_mut!(KB_BUF) as *mut u8).add(head) = up;
        KB_HEAD = next;
        if KB_WAITER != usize::MAX {
            let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
            (*s).wake(KB_WAITER);
            KB_WAITER = usize::MAX;
        }
    }
}

/// 从队列取最多 len 字节, 返回实际取数
unsafe fn kb_take(dst: *mut u8, len: usize) -> usize {
    let mut k = 0usize;
    while k < len {
        if KB_TAIL == KB_HEAD {
            break;
        }
        let b = *(core::ptr::addr_of!(KB_BUF) as *const u8).add(KB_TAIL);
        *dst.add(k) = b;
        KB_TAIL = (KB_TAIL + 1) % 32;
        k += 1;
    }
    k
}

#[allow(dead_code)]
// 调试探针: 打 cr3 + 四级表项 (排查完就删)
unsafe fn pte_dump(va: u64, tag: &[u8]) {
    let cr3 = asm_read_cr3() & 0x000F_FFFF_FFFF_F000;
    let mut line = [0u8; 240];
    let mut pos = 0usize;
    let mut put = |b: u8, pos: &mut usize| {
        *(line.as_mut_ptr().add(*pos)) = b;
        *pos += 1;
    };
    for &b in tag {
        put(b, &mut pos);
    }
    put(b' ', &mut pos);
    let mut hx = [0u8; 16];
    fmt_hex64(cr3, &mut hx);
    for &h in &hx {
        put(h, &mut pos);
    }
    put(b' ', &mut pos);
    let mut t = cr3;
    for lvl in 0..4usize {
        let e = *((t + (((va >> (39 - lvl * 9)) & 511) as u64) * 8) as *const u64);
        fmt_hex64(e, &mut hx);
        for &h in &hx {
            put(h, &mut pos);
        }
        put(b' ', &mut pos);
        t = e & 0x000F_FFFF_FFFF_F000;
        if e & 0x80 != 0 && lvl < 3 {
            put(b'L', &mut pos);
            put(b'P', &mut pos);
            break;
        }
    }
    put(b'\n', &mut pos);
    put(0, &mut pos);
    fb_print(line.as_ptr());
}

fn fmt_hex64(v: u64, out: &mut [u8; 16]) -> usize {
    let hex = b"0123456789ABCDEF";
    for i in 0..16 {
        out[i] = hex[((v >> (60 - i * 4)) & 0xF) as usize];
    }
    16
}

// 能力检查: 令牌得是 CAP_VMO 槽位才算帧缓冲的钥匙
fn fb_cap_check(token: u32) -> bool {
    unsafe {
        let c: *mut Option<capability::CSpace> = core::ptr::addr_of_mut!(CAPS);
        match (*c).as_mut() {
            Some(cs) => matches!(cs.lookup(token), Some((t, _)) if t == capability::CAP_VMO),
            None => false,
        }
    }
}

// int0x80 跳板落点: 返回新 rsp (0=原地), yield/阻塞靠它换栈
#[no_mangle]
pub extern "C" fn rust_syscall_entry(frame: *mut syscall::SyscallFrame) -> u64 {
    unsafe {
        let f = &mut *frame;
        // 4 = IPC_SEND: rdi=端点令牌 rsi=用户缓冲 rdx=长度, 过能力门才进通道
        // 60 = SYS_EXIT: 当前任务不再可调度, 切走
        if f.rax == 60 {
            let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
            let me = (*s).cur_index();
            (*s).exit_cur();
            // 归还装载窗口: 任务退出, 它占的窗口下个 spawn 可复用
            let wt = core::ptr::addr_of_mut!(WINTAB) as *mut usize;
            let mut k = 0usize;
            while k < NW {
                if *wt.add(k) == me {
                    *wt.add(k) = usize::MAX;
                    break;
                }
                k += 1;
            }
            f.rax = 0;
            return rust_schedule(frame as u64);
        }
        // 0 = SYS_READ (Linux ABI): rdi=fd rsi=buf rdx=len, 只接键盘队列 (fd 0)
        // 队列空: 登记等待者阻塞切走, 唤醒后回用户态 rax=0, 由调用方重试
        if f.rax == 0 {
            let len = if f.rdx > 64 { 64 } else { f.rdx } as usize;
            let dst = f.rsi as *mut u8;
            let got = kb_take(dst, len);
            if got == 0 {
                let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
                KB_WAITER = (*s).cur_index();
                (*s).block_cur();
                f.rax = 0;
                return rust_schedule(frame as u64);
            }
            f.rax = got as u64;
            return 0;
        }
        // 1 = SYS_WRITE 的 Linux 兼容形态: fd 1 → 行11, fd 2 → 行14 (不走能力门)
        if f.rax == 1 && (f.rdi == 1 || f.rdi == 2) {
            let row = if f.rdi == 2 { 14 } else { 11 };
            let len = if f.rdx > 64 { 64 } else { f.rdx } as usize;
            let src = f.rsi as *const u8;
            let mut k = 0usize;
            while k < len {
                let ch = *src.add(k);
                fb_putchar(k as i32, row, ch, 0x00FF00);
                k += 1;
            }
            f.rax = len as u64;
            return 0;
        }
        // 11 = SYS_SPAWN: rsi = tmpfs 文件名 (12B), rdx = 参数指针, r8 = 参数长
        // 读 ELF 镜像装载为 ring3 任务, 参数压进程栈
        if f.rax == 11 {
            f.rax = if fb_cap_check(f.rdi as u32) {
                let mut name = [0u8; tmpfs::NAME_LEN];
                let src = f.rsi as *const u8;
                let mut k = 0usize;
                while k < tmpfs::NAME_LEN {
                    *(name.as_mut_ptr().add(k)) = *src.add(k);
                    k += 1;
                }
                // 参数: 用户内存 ≤64B
                let mut arg = [0u8; 64];
                let mut alen = if f.r8 > 64 { 64 } else { f.r8 } as usize;
                let asrc = f.rdx as *const u8;
                let mut k = 0usize;
                while k < alen {
                    *(arg.as_mut_ptr().add(k)) = *asrc.add(k);
                    k += 1;
                }
                match FS.open(&name) {
                    Some(h) => {
                        let mut bin = [0u8; tmpfs::FILE_CAP];
                        let n = FS.read(h, 0, &mut bin);
                        if n == 0 {
                            u64::MAX
                        } else {
                            spawn_elf_image(&bin[..n], &arg[..alen])
                        }
                    }
                    None => u64::MAX,
                }
            } else {
                u64::MAX
            };
            return 0;
        }
        if f.rax == 4 {
            // 发送按令牌路由到对应通道, 成功后 rust_ipc_send 内部唤醒该通道的等待者
            f.rax = if ep_cap_check(f.rdi as u32) {
                let len = if f.rdx > 48 { 48 } else { f.rdx };
                rust_ipc_send(f.rdi as u32, f.rsi as *const u8, len) as u64
            } else {
                u64::MAX
            };
            return 0;
        }
        // 5 = IPC_RECV: 按令牌路由到对应通道, 收到就拷给用户, 空了就登记等待并阻塞
        if f.rax == 5 {
            let cid = match unsafe { ep_channel(f.rdi as u32) } {
                Some((c, _)) => c,
                None => {
                    f.rax = u64::MAX;
                    return 0;
                }
            };
            let r = rust_ipc_recv_ch(cid, f.rsi as *mut u8, f.rdx as *mut u64, f.r8 as *mut u32);
            if r == 0 {
                f.rax = 0;
                return 0;
            }
            // 通道空: 登记该通道的等待者, 阻塞, 切走
            let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
            let w = core::ptr::addr_of_mut!(CHAN_WAITER) as *mut usize;
            *w.add(cid) = (*s).cur_index();
            (*s).block_cur();
            f.rax = 0;
            return rust_schedule(frame as u64);
        }
        // 12 = YIELD: 主动让出 CPU
        if f.rax == 12 {
            f.rax = 0;
            return rust_schedule(frame as u64);
        }
        // 15 = BRK: rsi = 请求数, 返回清零区指针 (bump 堆, 16MB 起恒等窗口)
        if f.rax == 15 {
            let want = if f.rsi > 4096 { 4096 } else { f.rsi };
            let want = (want + 15) & !15u64;
            let brk = core::ptr::addr_of_mut!(USER_BRK);
            if want == 0 || *brk + want > USER_BRK_END {
                f.rax = u64::MAX;
                return 0;
            }
            let p = *brk;
            // 区域清零 (恒等或直映射) + 放行为用户可写
            let kv = if frame::DIRECT {
                (p + frame::DIRECT_BASE) as *mut u8
            } else {
                p as *mut u8
            };
            core::ptr::write_bytes(kv, 0, want as usize);
            patch_user_region(p);
            *brk += want;
            f.rax = p;
            return 0;
        }
        // 14 = SLEEP: rsi = 延迟 (tick, 100Hz), 到点由 tick 钩子唤醒
        if f.rax == 14 {
            let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
            let delay = if f.rsi > 6000 { 6000 } else { f.rsi };
            let now = tick_now() as u64;
            if (*s).sleep_push((*s).cur_index(), now + delay) {
                (*s).block_cur();
                f.rax = 0;
                return rust_schedule(frame as u64);
            }
            f.rax = u64::MAX; // 睡眠表满
            return 0;
        }
        // 6/7/8 = 文件打开/写/读: 文件令牌 = grant(CAP_VMO, 文件号)
        if f.rax == 6 {
            // OPEN: rdi=调用方任一有效令牌, rsi=名字指针 → rax=新文件令牌
            f.rax = if fb_cap_check(f.rdi as u32) {
                let mut name = [0u8; tmpfs::NAME_LEN];
                let src = f.rsi as *const u8;
                let mut k = 0usize;
                while k < tmpfs::NAME_LEN {
                    *(name.as_mut_ptr().add(k)) = *src.add(k);
                    k += 1;
                }
                match FS.open(&name) {
                    Some(h) => rust_cap_grant(syscall::CAP_FRAMEBUF, h as u32) as u64,
                    None => u64::MAX,
                }
            } else {
                u64::MAX
            };
            return 0;
        }
        if f.rax == 7 || f.rax == 8 {
            // WRF/RDF: rdi=文件令牌 rsi=用户缓冲 rdx=长度
            let fid = match (unsafe {
                let c: *mut Option<capability::CSpace> = core::ptr::addr_of_mut!(CAPS);
                match (*c).as_mut() {
                    Some(cs) => cs.lookup(f.rdi as u32),
                    None => None,
                }
            }) {
                Some((t, fid)) if t == syscall::CAP_FRAMEBUF => fid as usize,
                _ => {
                    f.rax = u64::MAX;
                    return 0;
                }
            };
            let len = if f.rdx > 64 { 64 } else { f.rdx } as usize;
            if f.rax == 7 {
                let mut tmp = [0u8; 64];
                let src = f.rsi as *const u8;
                let mut k = 0usize;
                while k < len {
                    *(tmp.as_mut_ptr().add(k)) = *src.add(k);
                    k += 1;
                }
                let sl = core::slice::from_raw_parts(tmp.as_ptr(), len);
                f.rax = FS.write(fid, 0, sl) as u64;
            } else {
                let mut tmp = [0u8; 64];
                let n = FS.read(fid, 0, &mut tmp);
                let dst = f.rsi as *mut u8;
                let mut k = 0usize;
                while k < n {
                    *dst.add(k) = *(tmp.as_ptr().add(k));
                    k += 1;
                }
                f.rax = n as u64;
            }
            return 0;
        }
        let mut fb = syscall::Framebuffer {
            base: FB_BASE,
            scan: FB_SCAN,
            w: FB_W,
            h: FB_H,
        };
        syscall::dispatch(f, fb_cap_check, &mut fb);
        0
    }
}

unsafe fn draw_count(col: i32, row: i32, v: u32, color: u32) {
    for x in 0..27i32 {
        for y in 0..16i32 {
            fb_putpixel(col * 9 + x, row * 17 + y, 0x000000);
        }
    }
    fb_putchar(col, row, b'0' + ((v / 100) % 10) as u8, color);
    fb_putchar(col + 1, row, b'0' + ((v / 10) % 10) as u8, color);
    fb_putchar(col + 2, row, b'0' + (v % 10) as u8, color);
}

// 线程体只做计数+自旋, 所有渲染收归 pit_tick 上下文, 避免多点位画屏打架
extern "C" fn thread_alpha() -> ! {
    loop {
        unsafe {
            CNT_A = CNT_A.wrapping_add(1);
        }
        for _ in 0..500000 {
            core::hint::spin_loop();
        }
    }
}

extern "C" fn thread_beta() -> ! {
    loop {
        unsafe {
            CNT_B = CNT_B.wrapping_add(1);
        }
        for _ in 0..500000 {
            core::hint::spin_loop();
        }
    }
}

// IPC 服务线程: 从通道收消息, 往返一次计一次数
static mut STACK_C: [u8; 16384] = [0; 16384];
static mut IPC_COUNT: u32 = 0;
static mut IPC_SCRATCH: [u8; 64] = [0; 64];
static mut CHAN_WAITER: [usize; NCHAN] = [usize::MAX; NCHAN]; // 每通道一个阻塞等待者

// fs 监视线程: 读 LOG 文件首字节上屏第 9 行, 爬升 = 用户写→内核存→内核读 全通
static mut STACK_D: [u8; 16384] = [0; 16384];

extern "C" fn thread_fswatch() -> ! {
    loop {
        unsafe {
            let mut b = [0u8; 1];
            let n = FS.read(0, 0, &mut b);
            if n == 1 {
                draw_count(7, 9, b[0] as u32, 0xFFFFFF);
            }
        }
        for _ in 0..300000 {
            core::hint::spin_loop();
        }
    }
}

extern "C" fn thread_ipc() -> ! {
    loop {
        unsafe {
            let mut len_out: u64 = 0;
            let mut cap_out: u32 = 0;
            let r = rust_ipc_recv(
                IPC_SCRATCH.as_mut_ptr(),
                &mut len_out,
                &mut cap_out,
            );
            if r == 0 {
                IPC_COUNT += 1;
                draw_count(3, 8, IPC_COUNT, 0xFF00FF);
            } else {
                // 通道空: 阻塞自己, 载荷一发消息就被唤醒
                let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
                CHAN_WAITER[0] = (*s).cur_index();
                (*s).block_cur();
            }
        }
        for _ in 0..100000 {
            core::hint::spin_loop();
        }
    }
}

// kernel_main 里 enable_interrupts 之前调一次
#[no_mangle]
pub extern "C" fn rust_tasks_init() -> i32 {
    unsafe {
        // 能力系统上线, 发两把钥匙: 帧缓冲(VMO) + IPC 端点(ENDPOINT)
        rust_cap_init();
        let tok = rust_cap_grant(syscall::CAP_FRAMEBUF, 0);
        let tp: *mut u32 = core::ptr::addr_of_mut!(PAYLOAD_TOKEN);
        *tp = tok;
        let ep = rust_cap_grant(capability::CAP_ENDPOINT, 0);
        let te: *mut u32 = core::ptr::addr_of_mut!(PAYLOAD_EP);
        *te = ep;
        let p2 = rust_cap_grant(syscall::CAP_FRAMEBUF, 0); // 任务B 的画屏钥匙
        let t2: *mut u32 = core::ptr::addr_of_mut!(PAYLOAD2_TOKEN);
        *t2 = p2;
        // VFS 翻译器两件套: 服务与客户端的过门钥匙 + 通道 1/2 端点
        let p3 = rust_cap_grant(syscall::CAP_FRAMEBUF, 0);
        let t3p: *mut u32 = core::ptr::addr_of_mut!(PAYLOAD3_TOKEN);
        *t3p = p3;
        let p4 = rust_cap_grant(syscall::CAP_FRAMEBUF, 0);
        let t4p: *mut u32 = core::ptr::addr_of_mut!(PAYLOAD4_TOKEN);
        *t4p = p4;
        let eq = rust_cap_grant(capability::CAP_ENDPOINT, 1);
        let eqp: *mut u32 = core::ptr::addr_of_mut!(PAYLOAD_EP_Q);
        *eqp = eq;
        let er = rust_cap_grant(capability::CAP_ENDPOINT, 2);
        let erp: *mut u32 = core::ptr::addr_of_mut!(PAYLOAD_EP_R);
        *erp = er;
        rust_ipc_channel_init();
        vmo_init(); // 共享内存池上线
        // 帧缓冲参数从 bootboot 抄 (fb_ptr@0x28, scanline/width/height)
        let bb = &*(0xffff_ffff_ffe0_0000usize as *const u8);
        let p = bb as *const u8;
        FB_BASE = core::ptr::read_unaligned(p.add(0x28) as *const u64);
        FB_SCAN = core::ptr::read_unaligned(p.add(0x3C) as *const u32) as u64;
        FB_W = core::ptr::read_unaligned(p.add(0x34) as *const u32) as i64;
        FB_H = core::ptr::read_unaligned(p.add(0x38) as *const u32) as i64;

        let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
        (*s).adopt_current(); // 现在跑着的 kernel_main/shell 是任务 0
        let sa = core::ptr::addr_of_mut!(STACK_A) as u64 + 16384;
        let sb = core::ptr::addr_of_mut!(STACK_B) as u64 + 16384;
        let a = (*s).spawn(thread_alpha as u64, sa);
        let b = (*s).spawn(thread_beta as u64, sb);
        let sc = core::ptr::addr_of_mut!(STACK_C) as u64 + 16384;
        let d = (*s).spawn(thread_ipc as u64, sc);
        let sd = core::ptr::addr_of_mut!(STACK_D) as u64 + 16384;
        let e5 = (*s).spawn(thread_fswatch as u64, sd);
        let ue = userland::user_payload_start as u64;
        let us = core::ptr::addr_of!(user_stack_top) as u64;
        let c = (*s).spawn_user(ue, us);
        let u2 = userland2::user2_start as u64;
        let c2 = (*s).spawn_user(u2, userland2::stack_top());
        // ELF 任务: 镜像先落 tmpfs 文件 "init", 再从文件读回解析装载
        // —— 打通 文件系统 → ELF 解析 → ring3 进程 的完整链路
        static mut ELF_IMG: [u8; 512] = [0; 512];
        static mut ELF_SEG: [u8; 0x1000] = [0; 0x1000];
        static mut ELF_STACK: [u8; 4096] = [0; 4096];
        let img = core::ptr::addr_of_mut!(ELF_IMG) as *mut u8;
        let seg_base = core::ptr::addr_of_mut!(ELF_SEG) as u64;
        // 段虚拟基址 = 段实际地址, 装载即按恒等地址拷贝, 无需额外建页
        let img_len = elfprog::build_image(core::slice::from_raw_parts_mut(img, 512), seg_base);
        // 1) 镜像落盘
        let fh = match FS.open(b"init") {
            Some(h) => h,
            None => return -4,
        };
        if FS.write(fh, 0, core::slice::from_raw_parts(img, img_len)) != img_len {
            return -4;
        }
        // spawn 演示程序: 写 "SPAWN OK" 到 fd 2 (行14) 后退出, 供 SYS_SPAWN 装载。
        // 参数版 (build_image_arg) 机制已就位但用户态读参尚待调试, 暂用固定消息版
        static mut HELLO_IMG: [u8; 256] = [0; 256];
        let hp = core::ptr::addr_of_mut!(HELLO_IMG) as *mut u8;
        let hn = elfprog::build_image_msg(
            core::slice::from_raw_parts_mut(hp, 256),
            0x0040_0000,
            2,
            *b"SPAWN OK",
        );
        let hh = match FS.open(b"hello") {
            Some(h) => h,
            None => return -4,
        };
        if FS.write(hh, 0, core::slice::from_raw_parts(hp, hn)) != hn {
            return -4;
        }
        // 2) 从文件读回, 解析并装载
        let bin_len = FS.read(fh, 0, core::slice::from_raw_parts_mut(img, 512));
        let elf_entry = elfprog::load(core::slice::from_raw_parts(img, bin_len), |va, bytes, _| {
            core::ptr::copy_nonoverlapping(bytes.as_ptr(), va as *mut u8, bytes.len());
            Ok(())
        });
        let elf_entry = match elf_entry {
            Ok(e) => e,
            Err(_) => return -2,
        };
        let elf_stack_top = core::ptr::addr_of_mut!(ELF_STACK) as u64 + 4096;
        let c3 = (*s).spawn_user(elf_entry, elf_stack_top);
        if c3.is_none() {
            return -3;
        }
        // VFS 翻译器服务 + 客户端: ring3 经通道 1/2 做 OPEN/WRITE/READ 往返
        let sv = (*s).spawn_user(userland3::srv_start as u64, userland3::stack_top());
        let cl = (*s).spawn_user(userland4::cli_start as u64, userland4::stack_top());
        if sv.is_none() || cl.is_none() {
            return -5;
        }
        // 键盘回显: SYS_READ 阻塞读, 有键唤醒
        let ec = (*s).spawn_user(userland5::echo_start as u64, userland5::stack_top());
        if ec.is_none() {
            return -6;
        }
        if a.is_none() || b.is_none() || c.is_none() || d.is_none() || e5.is_none() || c2.is_none()
        {
            return -1;
        }
        // 每任务一条独立中断栈, 调度切换时写 TSS.RSP0 — ring3 帧不再互相踩
        static mut IRQ_STACKS: [[u8; 4096]; 12] = [[0; 4096]; 12];
        for i in 0..(*s).n {
            let top = (core::ptr::addr_of_mut!(IRQ_STACKS) as u64) + ((i as u64) + 1) * 4096;
            (*s).set_irq_top(i, top);
        }
        // 直映射已上线, 页表修补在 Rust 里做
        patch_user_region(ue);
        patch_user_region(us);
        patch_user_region(seg_base);
        patch_user_region(elf_stack_top - 4096);
        0
    }
}

// pit_tick 取线程计数做单点渲染
#[no_mangle]
// pit_tick 每 tick 调 (kernel.c 首行), 计数 + 到点睡眠唤醒
#[no_mangle]
pub extern "C" fn rust_tick_hook() {
    unsafe {
        let t = core::ptr::addr_of_mut!(RUST_TICKS);
        *t += 1;
        let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
        (*s).sleep_wake(*t);
    }
}

// shell 的 run 命令入口: 按名装载 tmpfs ELF 为新任务, 返回槽号 (u64::MAX=败)
// dbg 标记: 0xC0=入口 0xC1=open 成功 0xCD=读到空 0xC3=装载完成 0xCE=open 败
#[no_mangle]
pub extern "C" fn rust_spawn_file(name: *const u8, arg: *const u8, arglen: u64) -> u64 {
    unsafe {
        let dbg = core::ptr::addr_of_mut!(SPAWN_DBG);
        *dbg = 0xC0;
        let mut nm = [0u8; tmpfs::NAME_LEN];
        let mut k = 0usize;
        // C 串语义: 首个 NUL 之后全按 0 填, 不把 cmd 缓冲的残渣带进文件名
        let mut nul = false;
        while k < tmpfs::NAME_LEN {
            let c = if nul { 0 } else { *name.add(k) };
            if c == 0 {
                nul = true;
            }
            *(nm.as_mut_ptr().add(k)) = c;
            k += 1;
        }
        let mut ab = [0u8; 64];
        let mut alen = if arglen > 64 { 64 } else { arglen as usize };
        // 参数同样按 C 串语义: 首个 NUL 截断
        let mut k = 0usize;
        while k < alen {
            let c = *arg.add(k);
            if c == 0 {
                alen = k;
                break;
            }
            *(ab.as_mut_ptr().add(k)) = c;
            k += 1;
        }
        match FS.open(&nm) {
            Some(h) => {
                *dbg = 0xC1;
                let mut bin = [0u8; tmpfs::FILE_CAP];
                let n = FS.read(h, 0, &mut bin);
                if n == 0 {
                    *dbg = 0xCD;
                    return u64::MAX;
                }
                *dbg = 0xC0 | (n as u32 & 0xFF); // 读到的长度低 8 位
                let r = spawn_elf_image(&bin[..n], &ab[..alen]);
                *dbg = 0xC3;
                r
            }
            None => {
                *dbg = 0xCE;
                u64::MAX
            }
        }
    }
}

// pit_tick 取线程计数做单点渲染
#[no_mangle]
pub extern "C" fn rust_thread_counts(which: u32) -> u32 {
    unsafe {
        if which == 0 {
            CNT_A
        } else {
            CNT_B
        }
    }
}

// pit_tick 每 tick 调, 传当前现场位置, 返回下一个该跑的 rsp (0=不动)
#[no_mangle]
pub extern "C" fn rust_schedule(frame: u64) -> u64 {
    unsafe {
        let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
        (*s).schedule(frame)
    }
}

// ============================================================
// 页表自举 / fork
// ============================================================

// 直映射不切 CR3: 在现行页表的空闲 PML4 槽上铺 1GB 大页窗口。
// 表帧走低分配线以上的恒等映射帧 (恒等可写), 铺完置 DIRECT 标志。
#[no_mangle]
pub extern "C" fn rust_direct_map_init(max_phys: u64) -> i32 {
    unsafe {
        if frame::DIRECT {
            return 0;
        }
        let fa = match fa_opt() {
            Some(f) => f,
            None => return -1,
        };
        let pml4 = asm_read_cr3() & 0x000F_FFFF_FFFF_F000;
        let slot = (frame::DIRECT_BASE >> 39) as usize & 511;
        let pml4v = pml4 as *mut u64;
        if *pml4v.add(slot) & 1 != 0 {
            return -2; // 槽被占
        }
        let pdpt = match frame::FrameSource::alloc_frame(fa) {
            Some(p) => p,
            None => return -3,
        };
        let pdptv = pdpt as *mut u64;
        core::ptr::write_bytes(pdptv as *mut u8, 0, 4096);
        let gigs = ((max_phys + 0x3FFF_FFFF) / 0x4000_0000).max(1);
        let mut g = 0u64;
        while g < gigs {
            let pd = match frame::FrameSource::alloc_frame(fa) {
                Some(p) => p,
                None => return -3,
            };
            let pdv = pd as *mut u64;
            core::ptr::write_bytes(pdv as *mut u8, 0, 4096);
            let mut j = 0u64;
            while j < 512 {
                let pa = (g << 30) | (j << 21);
                if pa >= max_phys {
                    break;
                }
                *pdv.add(j as usize) = pa | 1 | 2 | (1 << 7); // P|W|PS
                j += 1;
            }
            *pdptv.add(g as usize) = pd | 1 | 2;
            g += 1;
        }
        *pml4v.add(slot) = pdpt | 1 | 2;
        frame::DIRECT = true;
        0
    }
}

// 建自己的页表抄高半区+铺直映射然后切过去。max_phys=物理内存上限
#[no_mangle]
pub extern "C" fn rust_vm_bootstrap(max_phys: u64) -> i32 {
    unsafe {
        let fa = match fa_opt() {
            Some(f) => f,
            None => return -1,
        };
        let old = asm_read_cr3();
        match vmboot::build(fa, old, max_phys) {
            Some(np) => {
                asm_write_cr3(np);
                frame::DIRECT = true;
                let mut hx = [0u8; 16];
                let mut ln = [0u8; 64];
                let mut p = 0;
                for &b in b"[vm] old=" {
                    ln[p] = b;
                    p += 1;
                }
                fmt_hex64(old, &mut hx);
                for &h in &hx {
                    ln[p] = h;
                    p += 1;
                }
                for &b in b" new=" {
                    ln[p] = b;
                    p += 1;
                }
                fmt_hex64(np, &mut hx);
                for &h in &hx {
                    ln[p] = h;
                    p += 1;
                }
                ln[p] = b'\n';
                p += 1;
                ln[p] = 0;
                fb_print(ln.as_ptr());
                0
            }
            None => -1,
        }
    }
}

#[no_mangle]
pub extern "C" fn rust_user_aspace_new() -> u64 {
    unsafe {
        let fa = match fa_opt() {
            Some(f) => f,
            None => return u64::MAX,
        };
        aspace::new_user(fa, asm_read_cr3()).unwrap_or(u64::MAX)
    }
}

#[no_mangle]
pub extern "C" fn rust_fork_copy(out_pages: *mut u64) -> u64 {
    unsafe {
        let fa = match fa_opt() {
            Some(f) => f,
            None => return u64::MAX,
        };
        let src = asm_read_cr3() & pagetable::PHYS_MASK;
        let dst = match aspace::new_user(fa, src) {
            Some(p) => p,
            None => return u64::MAX,
        };
        let n = aspace::fork_deep(fa, src, dst);
        if !out_pages.is_null() {
            *out_pages = n as u64;
        }
        dst
    }
}

// ============================================================
// 报告输出
// ============================================================

fn fmt_dec(mut v: u64, out: &mut [u8; 20]) -> usize {
    if v == 0 {
        out[0] = b'0';
        return 1;
    }
    let mut tmp = [0u8; 20];
    let mut n = 0;
    while v > 0 && n < 20 {
        tmp[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    for i in 0..n {
        out[i] = tmp[n - 1 - i];
    }
    n
}

#[no_mangle]
pub extern "C" fn rust_mem_report() {
    let (free, total) = match unsafe { fa_opt() } {
        Some(fa) => (fa.free_count(), fa.total()),
        None => return,
    };
    let mut line = [0u8; 96];
    let mut pos = 0;

    let mut put = |s: &[u8]| {
        for &b in s {
            if pos < line.len() {
                line[pos] = b;
                pos += 1;
            }
        }
    };

    put(b"[rust] frames free=");
    let mut num = [0u8; 20];
    let n = fmt_dec(free as u64, &mut num);
    put(&num[..n]);
    put(b" / ");
    let n = fmt_dec(total as u64, &mut num);
    put(&num[..n]);
    put(b" (4K each)\n\0");

    unsafe { fb_print(line.as_ptr()) };
}
