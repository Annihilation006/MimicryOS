// ============================================================
// MimicryOS test 分支 · Rust 内核侧编译单元
// 维护：homo-ai (Hugo_YuGuo)
//
// 已具备：
//   1. 三方互操作桥（rust_cpu_report：Rust→C fb_print / Rust→asm 原语）
//   2. 物理帧管理（rust_mem_init：解析 BOOTBOOT 内存图 → 位图分配器，
//      详见 frame.rs 与设计文档 02-memory.md）
//      - rust_frame_alloc / rust_frame_free：C 侧也可直接调用的分配接口
//      - rust_mem_report：经 fb_print 输出内存统计
//
// 本文件不接管入口、不改动任何既有 C 代码；
// 上述函数何时接线调用，等 main 作者点头（见 TEST-BRANCH.md）。
//
// 编译：rustc --target x86_64-unknown-none --emit=obj（见 Makefile）
// ============================================================

#![no_std]

#[path = "frame.rs"]
mod frame;

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    // 裸机环境暂无 panic 输出通道；待与 fb_print 接线后补诊断信息
    loop {}
}

/* ---- main 分支既有 C 接口（kernel.c）---- */
extern "C" {
    fn fb_print(str: *const u8);
}

/* ---- cpu.asm 提供的汇编原语 ---- */
extern "C" {
    fn asm_read_cr0() -> u64;
    fn asm_read_cr3() -> u64;
    fn asm_cpuid(leaf: u32, subleaf: u32, out: *mut u32);
}

// ============================================================
// 第一部分：CPU 互操作桥（验证三方混合链接）
// ============================================================

/// 读取并回显关键 CPU 状态，验证 Rust → C / Rust → asm 桥可用。
/// 返回 CR0 当前值，便于调用方断言（分页位 PE/PG 等）。
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

    // cr3 当前仅作演示读取；后续物理帧/页表初始化会正式使用
    let _ = cr3;
    cr0
}

// ============================================================
// 第二部分：物理帧管理（02-memory.md）
// ============================================================

/* ---- BOOTBOOT 协议最小子集（完整定义见 bootboot.h，bzt，MIT）---- */

const BOOTBOOT_INFO_ADDR: usize = 0xffff_ffff_ffe0_0000;

#[repr(C, packed)]
struct MMapEnt {
    ptr: u64,
    size: u64, // 低 4 位是类型，其余为字节数（16 字节对齐单位）
}

#[repr(C, packed)]
struct Bootboot {
    magic: [u8; 4],      // "BOOT"
    size: u32,           // 结构体总长（含变长 mmap 表）
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
    _arch: [u8; 64],     // 平台相关区（此处不用）
    mmap: [MMapEnt; 1],  // 变长数组首地址
}

const MMAP_USED: u64 = 0;
const MMAP_FREE: u64 = 1;

/* 位图容量：管理上限 4 GiB 物理内存 = 1M 帧 = 16384 个 u64 = 128 KiB。
 * 静态放 .bss，无堆依赖；将来机器内存超限时改为按内存图动态截取。 */
const MAX_FRAMES: usize = 1 << 20;
const BM_WORDS: usize = MAX_FRAMES / frame::WORD_BITS;

static mut FRAME_BITMAP: [u64; BM_WORDS] = [0; BM_WORDS];
static mut FRAME_ALLOC: Option<frame::FrameAllocator> = None;

unsafe fn fa_opt() -> Option<&'static mut frame::FrameAllocator> {
    let p: *mut Option<frame::FrameAllocator> = core::ptr::addr_of_mut!(FRAME_ALLOC);
    (*p).as_mut()
}

/// 解析 BOOTBOOT 内存图并初始化物理帧分配器。
/// 返回空闲帧总数；BOOTBOOT magic 校验失败返回 usize::MAX。
/// 保守策略：默认全占用，仅 MMAP_FREE 区段放开，且跳过第 0 帧
/// （实模式 IVT / BIOS 数据区，历史包袱谁用谁知道）。
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

        // mmap 表从结构体第 128 字节开始，到 bootboot.size 为止
        let hdr = 128usize;
        let ent = core::mem::size_of::<MMapEnt>();
        let nents = ((bb.size as usize).saturating_sub(hdr)) / ent;
        let first: *const MMapEnt = core::ptr::addr_of!(bb.mmap) as *const MMapEnt;

        for i in 0..nents {
            let e: MMapEnt = core::ptr::read(first.add(i));
            let typ = e.size & 0xF;
            let bytes = e.size & !0xFu64;
            if typ != MMAP_FREE || bytes == 0 || typ == MMAP_USED {
                continue;
            }
            let start = e.ptr;
            let end = e.ptr.saturating_add(bytes);
            let f0 = ((start + frame::FRAME_SIZE - 1) / frame::FRAME_SIZE) as usize;
            let f1 = ((end + frame::FRAME_SIZE - 1) / frame::FRAME_SIZE) as usize;
            let f0 = f0.max(1); // 跳过第 0 帧
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

/// C 侧亦可直接调用的分配接口：成功返回物理帧号，失败返回 u64::MAX。
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

/// C 侧释放接口：0=成功，-1=未初始化/越界/重复释放。
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

/* 无 std 的十进制格式化（fb_print 只吃以 \0 结尾的字符串） */
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

/// 经既有 fb_print 输出内存统计（帧粒度统计，KiB 单位换算给调用方看）。
#[no_mangle]
pub extern "C" fn rust_mem_report() {
    let (free, total) = match unsafe { fa_opt() } {
        Some(fa) => (fa.free_count(), fa.total()),
        None => return,
    };
    let mut line = [0u8; 96];
    let mut pos = 0;

    let mut put = |s: &[u8]| {
        // 闭包捕获可变 pos —— 简单起见用显式循环展开
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
