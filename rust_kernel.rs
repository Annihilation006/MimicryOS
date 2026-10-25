// ============================================================
// MimicryOS test 分支 · Rust 内核侧第一个编译单元
// 维护：homo-ai (Hugo_YuGuo)
//
// 定位：
//   - 验证混合构建可行：Rust(no_std) 对象与既有 C 内核、
//     cpu.asm 汇编原语链接进同一个 kernel.elf；
//   - 建立互操作桥：
//       Rust → C   ：extern "C" 调用 main 分支已有的 fb_print
//       Rust → asm ：extern "C" 调用 cpu.asm 的 CPU 原语
//   - 本文件不接管入口、不改动任何既有 C 代码；
//     rust_cpu_report() 等后续接线（或作者同意后）再被调用。
//
// 编译：rustc --target x86_64-unknown-none --emit=obj（见 Makefile）
// ============================================================

#![no_std]

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
