//! 极简 ELF64 静态加载解析器（03-process 第一步）
//! 纯 Rust、no_std、无外部依赖，仅解析 ELF 头与 PT_LOAD 段

pub const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
pub const ELF_CLASS_64: u8 = 2;
pub const ELF_DATA_2LSB: u8 = 1;
pub const EM_X86_64: u16 = 0x3E;
pub const PT_LOAD: u32 = 1;

pub const PF_X: u32 = 1;
pub const PF_W: u32 = 2;
pub const PF_R: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfError {
    InvalidMagic,
    Not64Bit,
    NotLittleEndian,
    NotX86_64,
    TruncatedHeader,
    TruncatedProgramHeader,
    NoLoadableSegments,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Elf64Header {
    pub ident: [u8; 16],
    pub elf_type: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub ph_off: u64,
    pub sh_off: u64,
    pub flags: u32,
    pub eh_size: u16,
    pub ph_ent_size: u16,
    pub ph_num: u16,
    pub sh_ent_size: u16,
    pub sh_num: u16,
    pub sh_str_idx: u16,
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct Elf64ProgramHeader {
    pub seg_type: u32,
    pub flags: u32,
    pub offset: u64,
    pub vaddr: u64,
    pub paddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub align: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct LoadSegment {
    pub offset: u64,
    pub vaddr: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub flags: u32,
}

pub const MAX_SEGMENTS: usize = 8;

pub struct ElfFile<'a> {
    pub data: &'a [u8],
    pub entry: u64,
    pub segments: [Option<LoadSegment>; MAX_SEGMENTS],
    pub seg_count: usize,
}

impl<'a> ElfFile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, ElfError> {
        let eh_size = core::mem::size_of::<Elf64Header>();
        if data.len() < eh_size {
            return Err(ElfError::TruncatedHeader);
        }

        let hdr = unsafe { &*(data.as_ptr() as *const Elf64Header) };

        if hdr.ident[0..4] != ELF_MAGIC {
            return Err(ElfError::InvalidMagic);
        }
        if hdr.ident[4] != ELF_CLASS_64 {
            return Err(ElfError::Not64Bit);
        }
        if hdr.ident[5] != ELF_DATA_2LSB {
            return Err(ElfError::NotLittleEndian);
        }
        if hdr.machine != EM_X86_64 {
            return Err(ElfError::NotX86_64);
        }

        let ph_off = hdr.ph_off as usize;
        let ph_entsize = hdr.ph_ent_size as usize;
        let ph_num = hdr.ph_num as usize;

        if ph_entsize < core::mem::size_of::<Elf64ProgramHeader>() {
            return Err(ElfError::TruncatedProgramHeader);
        }

        let total_ph_size = ph_off.checked_add(ph_num.saturating_mul(ph_entsize))
            .ok_or(ElfError::TruncatedProgramHeader)?;
        if data.len() < total_ph_size {
            return Err(ElfError::TruncatedProgramHeader);
        }

        let mut segments: [Option<LoadSegment>; MAX_SEGMENTS] = [None; MAX_SEGMENTS];
        let mut seg_count = 0;

        for i in 0..ph_num {
            let ent_offset = ph_off + i * ph_entsize;
            let ph = unsafe { &*(data.as_ptr().add(ent_offset) as *const Elf64ProgramHeader) };

            if ph.seg_type == PT_LOAD {
                if seg_count < MAX_SEGMENTS {
                    segments[seg_count] = Some(LoadSegment {
                        offset: ph.offset,
                        vaddr: ph.vaddr,
                        filesz: ph.filesz,
                        memsz: ph.memsz,
                        flags: ph.flags,
                    });
                    seg_count += 1;
                }
            }
        }

        if seg_count == 0 {
            return Err(ElfError::NoLoadableSegments);
        }

        Ok(ElfFile {
            data,
            entry: hdr.entry,
            segments,
            seg_count,
        })
    }

    /// 将解析好的 ELF 文件各段载入指定目的空间中
    pub unsafe fn load_into<F>(&self, mut map_fn: F) -> Result<u64, ElfError>
    where
        F: FnMut(u64, &[u8], u32) -> Result<(), ElfError>,
    {
        for i in 0..self.seg_count {
            if let Some(seg) = self.segments[i] {
                let off = seg.offset as usize;
                let fsz = seg.filesz as usize;
                if off + fsz > self.data.len() {
                    return Err(ElfError::TruncatedProgramHeader);
                }
                let bytes = &self.data[off..off + fsz];
                map_fn(seg.vaddr, bytes, seg.flags)?;
            }
        }
        Ok(self.entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_dummy_elf() -> Vec<u8> {
        let eh_size = core::mem::size_of::<Elf64Header>();
        let ph_size = core::mem::size_of::<Elf64ProgramHeader>();
        let mut data = vec![0u8; eh_size + ph_size + 64];

        let hdr = unsafe { &mut *(data.as_mut_ptr() as *mut Elf64Header) };
        hdr.ident[0..4].copy_from_slice(&ELF_MAGIC);
        hdr.ident[4] = ELF_CLASS_64;
        hdr.ident[5] = ELF_DATA_2LSB;
        hdr.machine = EM_X86_64;
        hdr.entry = 0x400000;
        hdr.ph_off = eh_size as u64;
        hdr.eh_size = eh_size as u16;
        hdr.ph_ent_size = ph_size as u16;
        hdr.ph_num = 1;

        let ph = unsafe { &mut *(data.as_mut_ptr().add(eh_size) as *mut Elf64ProgramHeader) };
        ph.seg_type = PT_LOAD;
        ph.flags = PF_R | PF_X;
        ph.offset = (eh_size + ph_size) as u64;
        ph.vaddr = 0x400000;
        ph.filesz = 16;
        ph.memsz = 16;
        ph.align = 0x1000;

        data
    }

    #[test]
    fn test_parse_valid_elf() {
        let data = make_dummy_elf();
        let elf = ElfFile::parse(&data).expect("Failed to parse valid ELF");
        assert_eq!(elf.entry, 0x400000);
        assert_eq!(elf.seg_count, 1);
        let seg = elf.segments[0].unwrap();
        assert_eq!(seg.vaddr, 0x400000);
        assert_eq!(seg.filesz, 16);
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = make_dummy_elf();
        data[0] = 0x00;
        assert_eq!(ElfFile::parse(&data).err(), Some(ElfError::InvalidMagic));
    }
}
