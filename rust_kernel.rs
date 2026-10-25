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

unsafe fn fa_opt() -> Option<&'static mut frame::FrameAllocator> {
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
            let f0 = f0.max(1); // 跳过第 0 帧（IVT/BIOS 数据区）
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
const NVMO: usize = 8;

static mut CAP_NODES: [capability::CapNode; NCAPS] =
    [capability::CapNode { obj_type: 0, obj_id: 0, parent: 0, first_child: capability::NO_SLOT, next_sibling: capability::NO_SLOT }; NCAPS];
static mut CAP_USED: [bool; NCAPS] = [false; NCAPS];
static mut CAP_FREE: [u32; NCAPS] = [0; NCAPS];
static mut CAPS: Option<capability::CSpace> = None;

static mut CH_STORAGE: [ipc::Message; NCHMSG] =
    [ipc::Message { cap: 0, len: 0, data: [0; ipc::MSG_PAYLOAD] }; NCHMSG];
static mut CHAN: Option<ipc::Channel> = None;

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
        let p: *mut Option<ipc::Channel> = core::ptr::addr_of_mut!(CHAN);
        *p = Some(ipc::Channel::new(
            core::ptr::addr_of_mut!(CH_STORAGE) as *mut ipc::Message,
            NCHMSG,
        ));
    }
    0
}

#[no_mangle]
pub extern "C" fn rust_ipc_send(cap: u32, buf: *const u8, len: u64) -> i32 {
    let len = len as usize;
    if len > ipc::MSG_PAYLOAD {
        return -2;
    }
    let ch = match unsafe { (&mut *core::ptr::addr_of_mut!(CHAN)).as_mut() } {
        Some(c) => c,
        None => return -3,
    };
    let mut m = ipc::Message { cap, len: len as u16, data: [0; ipc::MSG_PAYLOAD] };
    unsafe {
        let dst = m.data.as_mut_ptr();
        for i in 0..len {
            *dst.add(i) = *buf.add(i);
        }
    }
    match ch.send(&m) {
        Ok(()) => 0,
        Err(_) => -1,
    }
}

#[no_mangle]
pub extern "C" fn rust_ipc_recv(buf: *mut u8, len_out: *mut u64, cap_out: *mut u32) -> i32 {
    let ch = match unsafe { (&mut *core::ptr::addr_of_mut!(CHAN)).as_mut() } {
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

extern "C" {
    fn fb_putchar(cx: i32, cy: i32, c: u8, color: u32);
    fn fb_putpixel(x: i32, y: i32, color: u32);
    static user_payload_start: u8; // usermode.asm 的符号, 地址即入口
    static user_stack_top: u8;
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

// kernel_main 里 enable_interrupts 之前调一次
#[no_mangle]
pub extern "C" fn rust_tasks_init() -> i32 {
    unsafe {
        let s: *mut task::Scheduler = core::ptr::addr_of_mut!(SCHED);
        (*s).adopt_current(); // 现在跑着的 kernel_main/shell 是任务 0
        let sa = core::ptr::addr_of_mut!(STACK_A) as u64 + 16384;
        let sb = core::ptr::addr_of_mut!(STACK_B) as u64 + 16384;
        let a = (*s).spawn(thread_alpha as u64, sa);
        let b = (*s).spawn(thread_beta as u64, sb);
        let ue = core::ptr::addr_of!(user_payload_start) as u64;
        let us = core::ptr::addr_of!(user_stack_top) as u64;
        let c = (*s).spawn_user(ue, us);
        if a.is_none() || b.is_none() || c.is_none() {
            return -1;
        }
        0
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

// 建自己的页表抄高半区+铺直映射然后切过去。max_phys=物理内存上限
#[no_mangle]
pub extern "C" fn rust_vm_bootstrap(max_phys: u64) -> i32 {
    unsafe {
        let fa = match fa_opt() {
            Some(f) => f,
            None => return -1,
        };
        match vmboot::build(fa, asm_read_cr3(), max_phys) {
            Some(np) => {
                asm_write_cr3(np);
                frame::DIRECT = true;
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
