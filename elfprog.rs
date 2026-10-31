//! 内嵌的最小 ELF 用户程序与其装载入口
//! 镜像内容：向指定 fd 写一行 8 字节文本后调用 SYS_EXIT(60)。
//! 代码 RIP 相对寻址 → 位置无关, 运行时可装载到任意页对齐地址;
//! 段的虚拟基址仅用于镜像里的 p_vaddr/入口字段的演练, 装载方按差值平移。
//!
//! 裸机构建注意：所有字节操作都在定长局部数组上以常量范围完成，
//! 出口用一次 copy_nonoverlapping 拷到调用方缓冲——运行期索引会
//! 引用 panic_bounds_check，裸机链接不了。

use crate::elf::{ElfError, ElfFile, Elf64Header, Elf64ProgramHeader, ELF_CLASS_64, ELF_DATA_2LSB, ELF_MAGIC, EM_X86_64, PF_R, PF_X, PT_LOAD};

pub const MSG_LEN: usize = 8;

const EH: usize = core::mem::size_of::<Elf64Header>();
const PH: usize = core::mem::size_of::<Elf64ProgramHeader>();
pub const CODE_OFF: usize = EH + PH;
// mov eax(5) + mov edi(5) + lea rip相对(7) + mov edx(5) + int(2) + mov eax(5) + int(2) + jmp(2)
pub const CODE_LEN: usize = 33;
pub const IMG_LEN: usize = CODE_OFF + CODE_LEN + MSG_LEN;

// lea rsi,[rip+disp32] 的 disp: 指令止于偏移 17, 消息在偏移 CODE_LEN → 差 = CODE_LEN-17
const MSG_DISP: i32 = (CODE_LEN - 17) as i32;
const _: () = assert!(MSG_DISP == 16, "lea disp 布局变了");

/// 机器码模板（offset 6 = fd）:
///   mov eax, 1          ; SYS_WRITE
///   mov edi, fd         ; 1 → 行11, 2 → 行14
///   lea rsi, [rip+d]    ; 消息 (位置无关)
///   mov edx, 8          ; 长度
///   int 0x80
///   mov eax, 60         ; SYS_EXIT
///   int 0x80
///   jmp $
const CODE_TMPL: [u8; CODE_LEN] = [
    0xB8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1
    0xBF, 0x01, 0x00, 0x00, 0x00, // mov edi, fd (offset 6)
    0x48, 0x8D, 0x35, 16, 0, 0, 0, // lea rsi, [rip+16]
    0xBA, MSG_LEN as u8, 0, 0, 0, // mov edx, 8
    0xCD, 0x80, // int 0x80
    0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60
    0xCD, 0x80, // int 0x80
    0xEB, 0xFE, // jmp $
];

fn code_bytes(fd: u8) -> [u8; CODE_LEN] {
    let mut c = CODE_TMPL;
    c[6] = fd;
    c
}

/// 把镜像写入 out（段虚拟基址 base, 写往 fd, 消息 8 字节定长），返回总长度。
/// 要求 out.len() >= IMG_LEN (161)。
pub fn build_image_msg(out: &mut [u8], base: u64, fd: u8, msg: [u8; MSG_LEN]) -> usize {
    let mut buf = [0u8; IMG_LEN];
    let code = code_bytes(fd);

    buf[0..4].copy_from_slice(&ELF_MAGIC);
    buf[4] = ELF_CLASS_64;
    buf[5] = ELF_DATA_2LSB;
    buf[6] = 1; // EV_CURRENT
    buf[16..18].copy_from_slice(&2u16.to_le_bytes()); // ET_EXEC
    buf[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
    buf[20..24].copy_from_slice(&1u32.to_le_bytes());
    buf[24..32].copy_from_slice(&(base + CODE_OFF as u64).to_le_bytes()); // e_entry
    buf[32..40].copy_from_slice(&(EH as u64).to_le_bytes()); // e_phoff
    buf[52..54].copy_from_slice(&(EH as u16).to_le_bytes()); // e_ehsize
    buf[54..56].copy_from_slice(&(PH as u16).to_le_bytes()); // e_phentsize
    buf[56..58].copy_from_slice(&1u16.to_le_bytes()); // e_phnum

    let p = EH;
    buf[p..p + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    buf[p + 4..p + 8].copy_from_slice(&(PF_R | PF_X).to_le_bytes());
    buf[p + 8..p + 16].copy_from_slice(&0u64.to_le_bytes()); // p_offset
    buf[p + 16..p + 24].copy_from_slice(&base.to_le_bytes()); // p_vaddr
    buf[p + 24..p + 32].copy_from_slice(&base.to_le_bytes()); // p_paddr
    buf[p + 32..p + 40].copy_from_slice(&(IMG_LEN as u64).to_le_bytes()); // p_filesz
    buf[p + 40..p + 48].copy_from_slice(&(IMG_LEN as u64).to_le_bytes()); // p_memsz
    buf[p + 48..p + 56].copy_from_slice(&0x1000u64.to_le_bytes());

    buf[CODE_OFF..CODE_OFF + CODE_LEN].copy_from_slice(&code);
    buf[CODE_OFF + CODE_LEN..IMG_LEN].copy_from_slice(&msg);

    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), out.as_mut_ptr(), IMG_LEN);
    }
    IMG_LEN
}

/// 缺省消息 "ELF OK\n"
pub fn build_image(out: &mut [u8], base: u64) -> usize {
    build_image_msg(out, base, 1, *b"ELF OK\n\0")
}

/// 装载：逐段回调 map_fn(目的虚拟地址, 段内容, 段权限)，返回入口地址。
pub fn load<F>(image: &[u8], map_fn: F) -> Result<u64, ElfError>
where
    F: FnMut(u64, &[u8], u32) -> Result<(), ElfError>,
{
    let elf = ElfFile::parse(image)?;
    unsafe { elf.load_into(map_fn) }
}

// ---- 参数版: 程序从栈页底部固定地址取 argv, write(fd=rdi 处) 后退出 ----
// 内核约定: 栈页底部 base+0x1000 = 参数长度, base+0x1008 = 参数指针。
// 程序用 RIP 相对读 (disp32 无射程限制), 代码在 base+CODE_OFF:
//   lea 处止于 CODE_OFF+7 → dispA = 0x1000 - CODE_OFF - 7
//   第二条止于 CODE_OFF+14 → dispB = 0x1000 - CODE_OFF - 14
pub const ARG_CODE_LEN: usize = 41;
const DISP_A: i32 = (0x1000 - CODE_OFF - 7) as i32;
const DISP_B: i32 = (0x1000 - CODE_OFF - 14) as i32;

/// 机器码:
///   mov rax, [rip+DISP_A] ; len
///   mov rdx, [rip+DISP_B] ; ptr
///   mov rsi, rdx
///   mov rdx, rax
///   mov eax, 1 / mov edi, fd / int 0x80 / mov eax, 60 / int 0x80 / jmp $
const ARG_CODE: [u8; ARG_CODE_LEN] = [
    0x48, 0x8B, 0x05,
    (DISP_A & 0xFF) as u8,
    ((DISP_A >> 8) & 0xFF) as u8,
    ((DISP_A >> 16) & 0xFF) as u8,
    ((DISP_A >> 24) & 0xFF) as u8, // mov rax, [rip+DISP_A]
    0x48, 0x8B, 0x15,
    (DISP_B & 0xFF) as u8,
    ((DISP_B >> 8) & 0xFF) as u8,
    ((DISP_B >> 16) & 0xFF) as u8,
    ((DISP_B >> 24) & 0xFF) as u8, // mov rdx, [rip+DISP_B]
    0x48, 0x89, 0xD6, // mov rsi, rdx
    0x48, 0x89, 0xC2, // mov rdx, rax
    0xB8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1
    0xBF, 0x02, 0x00, 0x00, 0x00, // mov edi, 2
    0xCD, 0x80, // int 0x80
    0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60
    0xCD, 0x80, // int 0x80
    0xEB, 0xFE, // jmp $
];

/// 参数版镜像: 入口即 write(fd, argv, len) 后退出 (fd 由调用方定,
/// 内核按 fd 选输出行: 1→行11, 2→行14, 3→行20)。IMG_LEN 同为 161。
pub fn build_image_arg(out: &mut [u8], base: u64, fd: u8) -> usize {
    let mut buf = [0u8; IMG_LEN];
    buf[0..4].copy_from_slice(&ELF_MAGIC);
    buf[4] = ELF_CLASS_64;
    buf[5] = ELF_DATA_2LSB;
    buf[6] = 1;
    buf[16..18].copy_from_slice(&2u16.to_le_bytes());
    buf[18..20].copy_from_slice(&EM_X86_64.to_le_bytes());
    buf[20..24].copy_from_slice(&1u32.to_le_bytes());
    buf[24..32].copy_from_slice(&(base + CODE_OFF as u64).to_le_bytes());
    buf[32..40].copy_from_slice(&(EH as u64).to_le_bytes());
    buf[52..54].copy_from_slice(&(EH as u16).to_le_bytes());
    buf[54..56].copy_from_slice(&(PH as u16).to_le_bytes());
    buf[56..58].copy_from_slice(&1u16.to_le_bytes());

    let p = EH;
    buf[p..p + 4].copy_from_slice(&PT_LOAD.to_le_bytes());
    buf[p + 4..p + 8].copy_from_slice(&(PF_R | PF_X).to_le_bytes());
    buf[p + 8..p + 16].copy_from_slice(&0u64.to_le_bytes());
    buf[p + 16..p + 24].copy_from_slice(&base.to_le_bytes());
    buf[p + 24..p + 32].copy_from_slice(&base.to_le_bytes());
    buf[p + 32..p + 40].copy_from_slice(&(IMG_LEN as u64).to_le_bytes());
    buf[p + 40..p + 48].copy_from_slice(&(IMG_LEN as u64).to_le_bytes());
    buf[p + 48..p + 56].copy_from_slice(&0x1000u64.to_le_bytes());
    buf[CODE_OFF..CODE_OFF + ARG_CODE_LEN].copy_from_slice(&ARG_CODE);
    // fd 立即数在 mov edi,imm32 的操作数位 (指令起于 CODE_OFF+25, imm 在 +26)
    buf[CODE_OFF + 26..CODE_OFF + 30].copy_from_slice(&(fd as u32).to_le_bytes());

    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), out.as_mut_ptr(), IMG_LEN);
    }
    IMG_LEN
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_parses_and_entry_points_at_code() {
        let base = 0x0040_0000u64;
        let mut buf = [0u8; 256];
        let n = build_image(&mut buf, base);
        let mut seen = 0usize;
        let mut got_va = 0u64;
        let entry = load(&buf[..n], |va, bytes, _| {
            got_va = va;
            seen = bytes.len();
            Ok(())
        })
        .unwrap();
        assert_eq!(got_va, base);
        assert_eq!(entry, base + CODE_OFF as u64);
        assert_eq!(seen, n);
    }

    #[test]
    fn image_code_ends_with_exit_and_spin() {
        let mut buf = [0u8; 256];
        let n = build_image(&mut buf, 0x0040_0000);
        let code = &buf[CODE_OFF..CODE_OFF + CODE_LEN];
        assert_eq!(&code[10..12], &[0x48, 0x8D]); // lea rip 相对
        assert_eq!(&code[CODE_LEN - 2..], &[0xEB, 0xFE]);
        assert_eq!(&buf[CODE_OFF + CODE_LEN..n], b"ELF OK\n\0");
    }

    #[test]
    fn msg_variant_carries_fd_and_text() {
        let mut buf = [0u8; 256];
        let n = build_image_msg(&mut buf, 0x0040_0000, 2, *b"SPAWN OK");
        let code = &buf[CODE_OFF..CODE_OFF + CODE_LEN];
        assert_eq!(code[6], 2); // fd=2
        assert_eq!(&buf[CODE_OFF + CODE_LEN..n], b"SPAWN OK");
        // lea 的 disp 指向消息起点: 指令止于 17, 目标在 CODE_LEN
        let disp = i32::from_le_bytes([code[13], code[14], code[15], code[16]]);
        assert_eq!(disp as usize, CODE_LEN - 17);
        // 位置无关: 换基址装载, 入口随差值平移
        let entry = load(&buf[..n], |_, _, _| Ok(())).unwrap();
        assert_eq!(entry, 0x0040_0000 + CODE_OFF as u64);
    }

    #[test]
    fn arg_variant_reads_stack() {
        let mut buf = [0u8; 256];
        let n = build_image_arg(&mut buf, 0x0040_0000, 2);
        assert_eq!(n, IMG_LEN);
        let code = &buf[CODE_OFF..CODE_OFF + ARG_CODE_LEN];
        // RIP 相对取参: 两条 lea/mov 的 disp 指向栈页底部
        assert_eq!(&code[0..3], &[0x48, 0x8B, 0x05]);
        assert_eq!(&code[7..10], &[0x48, 0x8B, 0x15]);
        let da = i32::from_le_bytes([code[3], code[4], code[5], code[6]]);
        let db = i32::from_le_bytes([code[10], code[11], code[12], code[13]]);
        // 指令止于 CODE_OFF+7 / +14, 目标 = base+0x1000 / +0x1008
        assert_eq!(da as usize, 0x1000 - CODE_OFF - 7);
        assert_eq!(db as usize, 0x1000 - CODE_OFF - 14);
        // fd 立即数随参数可变: mov edi, fd
        assert_eq!(&code[25..30], &[0xBF, 2, 0, 0, 0]);
        let entry = load(&buf[..n], |_, _, _| Ok(())).unwrap();
        assert_eq!(entry, 0x0040_0000 + CODE_OFF as u64);
    }
}
