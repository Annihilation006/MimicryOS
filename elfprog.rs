//! 内嵌的最小 ELF 用户程序与其装载入口
//! 镜像内容：向 fd 1 写一行文本后调用 SYS_EXIT(60)。
//! 段的虚拟基址由调用方给出（内核侧为 .bss 中已放行的用户页）。
//!
//! 裸机构建注意：所有字节操作都在定长局部数组上以常量范围完成，
//! 出口用一次 copy_nonoverlapping 拷到调用方缓冲——运行期索引会
//! 引用 panic_bounds_check，裸机链接不了。

use crate::elf::{ElfError, ElfFile, Elf64Header, Elf64ProgramHeader, ELF_CLASS_64, ELF_DATA_2LSB, ELF_MAGIC, EM_X86_64, PF_R, PF_X, PT_LOAD};

pub const MSG: &[u8] = b"ELF OK\n";

const EH: usize = core::mem::size_of::<Elf64Header>();
const PH: usize = core::mem::size_of::<Elf64ProgramHeader>();
const CODE_OFF: usize = EH + PH;
const CODE_LEN: usize = 36;
const IMG_LEN: usize = CODE_OFF + CODE_LEN + MSG.len();

/// 机器码模板（offset 12..20 = msg 地址, 21..25 = msg 长度 占位）:
///   mov eax, 1          ; SYS_WRITE
///   mov edi, 1          ; fd = 1
///   mov rsi, msg        ; 消息地址
///   mov edx, len        ; 长度
///   int 0x80
///   mov eax, 60         ; SYS_EXIT
///   int 0x80
///   jmp $
const CODE_TMPL: [u8; CODE_LEN] = [
    0xB8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1
    0xBF, 0x01, 0x00, 0x00, 0x00, // mov edi, 1
    0x48, 0xBE, // mov rsi, imm64
    0, 0, 0, 0, 0, 0, 0, 0, //   <- msg 地址
    0xBA, 0, 0, 0, 0, // mov edx, imm32
    0xCD, 0x80, // int 0x80
    0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60
    0xCD, 0x80, // int 0x80
    0xEB, 0xFE, // jmp $
];

fn code_bytes(msg_addr: u64) -> [u8; CODE_LEN] {
    let mut c = CODE_TMPL;
    c[12..20].copy_from_slice(&msg_addr.to_le_bytes());
    c[21..25].copy_from_slice(&(MSG.len() as u32).to_le_bytes());
    c
}

/// 把镜像写入 out（段虚拟基址为 base），返回总长度。
/// 要求 out.len() >= IMG_LEN (127)。
pub fn build_image(out: &mut [u8], base: u64) -> usize {
    let mut buf = [0u8; IMG_LEN];
    let code = code_bytes(base + (CODE_OFF + CODE_LEN) as u64);

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
    buf[CODE_OFF + CODE_LEN..IMG_LEN].copy_from_slice(MSG);

    unsafe {
        core::ptr::copy_nonoverlapping(buf.as_ptr(), out.as_mut_ptr(), IMG_LEN);
    }
    IMG_LEN
}

/// 装载：逐段回调 map_fn(目的虚拟地址, 段内容, 段权限)，返回入口地址。
pub fn load<F>(image: &[u8], map_fn: F) -> Result<u64, ElfError>
where
    F: FnMut(u64, &[u8], u32) -> Result<(), ElfError>,
{
    let elf = ElfFile::parse(image)?;
    unsafe { elf.load_into(map_fn) }
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
        assert_eq!(&code[10..12], &[0x48, 0xBE]);
        assert_eq!(&code[CODE_LEN - 2..], &[0xEB, 0xFE]);
        assert_eq!(&buf[CODE_OFF + CODE_LEN..n], MSG);
    }
}
