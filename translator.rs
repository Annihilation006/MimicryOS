//! 用户态 VFS 翻译器（01/06-vfs.md）：一切皆翻译器
//! 协议: 请求 FsRequest(28B, 写操作扩展到 44B) → 应答 FsResponse(40B), 一条 IPC 消息内联
//! serve() 是纯逻辑核: 只依赖 FsBack 接口, 内核线程和宿主测试共用

pub const FS_OP_OPEN: u16 = 1;
pub const FS_OP_READ: u16 = 2;
pub const FS_OP_WRITE: u16 = 3;
pub const FS_OP_STAT: u16 = 4;

pub const REQ_SIZE: usize = 28;
pub const REQ_DATA_SIZE: usize = 44; // 固定段 28 + 写载荷 16
pub const RESP_SIZE: usize = 40;
pub const NAME_FIELD: usize = 16;
pub const DATA_FIELD: usize = 32;
pub const WRITE_FIELD: usize = 16;

/// 翻译器背后的文件系统 (tmpfs 正好长这样, 宿主测试用替身)
pub trait FsBack {
    fn open(&mut self, name: &[u8]) -> Option<usize>;
    fn read(&self, h: usize, off: usize, dst: &mut [u8]) -> usize;
    fn write(&mut self, h: usize, off: usize, src: &[u8]) -> usize;
    fn size(&self, h: usize) -> usize;
}

/// FsRequest 布局: op u16 @0, fd u16 @2, offset u32 @4, len u32 @8, name[16] @12,
/// 写载荷 data[16] @28 (只有 WRITE 用, 其余操作请求体止于 28B)
pub fn build_request(op: u16, fd: u16, offset: u32, len: u32, name: &[u8], out: &mut [u8; REQ_SIZE]) {
    for b in out.iter_mut() {
        *b = 0;
    }
    out[0..2].copy_from_slice(&op.to_le_bytes());
    out[2..4].copy_from_slice(&fd.to_le_bytes());
    out[4..8].copy_from_slice(&offset.to_le_bytes());
    out[8..12].copy_from_slice(&len.to_le_bytes());
    let n = if name.len() < NAME_FIELD { name.len() } else { NAME_FIELD };
    out[12..12 + n].copy_from_slice(&name[..n]);
}

/// 带写载荷的完整请求 (WRITE 用): 28B 固定段 + 16B 载荷
pub fn build_request_data(
    op: u16,
    fd: u16,
    offset: u32,
    len: u32,
    name: &[u8],
    payload: &[u8],
    out: &mut [u8; REQ_DATA_SIZE],
) {
    let mut fixed = [0u8; REQ_SIZE];
    build_request(op, fd, offset, len, name, &mut fixed);
    out[..REQ_SIZE].copy_from_slice(&fixed);
    let n = if payload.len() < WRITE_FIELD { payload.len() } else { WRITE_FIELD };
    out[REQ_SIZE..REQ_SIZE + n].copy_from_slice(&payload[..n]);
}

#[inline(always)]
pub fn req_op(req: &[u8]) -> u16 {
    u16::from_le_bytes([req[0], req[1]])
}
#[inline(always)]
pub fn req_fd(req: &[u8]) -> u16 {
    u16::from_le_bytes([req[2], req[3]])
}
#[inline(always)]
pub fn req_offset(req: &[u8]) -> u32 {
    u32::from_le_bytes([req[4], req[5], req[6], req[7]])
}
#[inline(always)]
pub fn req_len(req: &[u8]) -> u32 {
    u32::from_le_bytes([req[8], req[9], req[10], req[11]])
}
#[inline(always)]
pub fn req_name(req: &[u8], i: usize) -> u8 {
    req[12 + i]
}
#[inline(always)]
pub fn req_payload(req: &[u8], i: usize) -> u8 {
    // 调用方保证 REQ_SIZE + i < req.len() (serve 的 WRITE 分支钳过 take);
    // 裸指针读: 运行期索引的越界检查符号裸机链接不了
    unsafe { *req.as_ptr().add(REQ_SIZE + i) }
}

/// FsResponse 布局: status i32 @0, len u32 @4, data[32] @8
#[inline(always)]
pub fn resp_status(resp: &[u8]) -> i32 {
    i32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]])
}
#[inline(always)]
pub fn resp_len(resp: &[u8]) -> u32 {
    u32::from_le_bytes([resp[4], resp[5], resp[6], resp[7]])
}
#[inline(always)]
pub fn resp_data(resp: &[u8], i: usize) -> u8 {
    resp[8 + i]
}

/// 处理一条请求, 应答字节写入 resp, 返回应答总长
pub fn serve(req: &[u8], resp: &mut [u8; RESP_SIZE], fs: &mut dyn FsBack) -> usize {
    let mut status: i32 = -1;
    let mut n: usize = 0;
    if req.len() >= REQ_SIZE {
        match req_op(req) {
            FS_OP_OPEN => {
                let mut name = [0u8; 12];
                let src = &req[12..12 + NAME_FIELD];
                let k = if src.len() < 12 { src.len() } else { 12 };
                name[..k].copy_from_slice(&src[..k]);
                match fs.open(&name) {
                    Some(h) => status = h as i32,
                    None => status = -1,
                }
            }
            FS_OP_READ => {
                let fd = req_fd(req) as usize;
                let off = req_offset(req) as usize;
                let want = req_len(req) as usize;
                let mut dst = [0u8; DATA_FIELD];
                let take = if want < DATA_FIELD { want } else { DATA_FIELD };
                n = fs.read(fd, off, &mut dst[..take]);
                status = 0;
                let base = 8usize;
                unsafe {
                    core::ptr::copy_nonoverlapping(dst.as_ptr(), resp.as_mut_ptr().add(base), n);
                }
            }
            FS_OP_WRITE => {
                // 载荷可要没有: take 同时钳在请求实际长度上, 索引才能免越界检查
                let avail = if req.len() > REQ_SIZE { req.len() - REQ_SIZE } else { 0 };
                let want = req_len(req) as usize;
                let mut take = if want < WRITE_FIELD { want } else { WRITE_FIELD };
                if take > avail {
                    take = avail;
                }
                let fd = req_fd(req) as usize;
                let off = req_offset(req) as usize;
                let mut src = [0u8; WRITE_FIELD];
                let mut k = 0usize;
                while k < take {
                    src[k] = req_payload(req, k);
                    k += 1;
                }
                n = fs.write(fd, off, &src[..take]);
                status = 0;
            }
            FS_OP_STAT => {
                n = fs.size(req_fd(req) as usize);
                status = 0;
            }
            _ => {
                status = -1;
            }
        }
    }
    resp[0..4].copy_from_slice(&status.to_le_bytes());
    resp[4..8].copy_from_slice(&(n as u32).to_le_bytes());
    RESP_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemFs {
        names: [Option<Vec<u8>>; 4],
        files: [Option<Vec<u8>>; 4],
    }
    impl MemFs {
        fn new() -> MemFs {
            MemFs { names: [None, None, None, None], files: [None, None, None, None] }
        }
    }
    impl FsBack for MemFs {
        // 语义与 tmpfs 一致: 按名找, 没有才建; 请求名是零填充定长串, 先裁尾零
        fn open(&mut self, name: &[u8]) -> Option<usize> {
            let mut trimmed: Vec<u8> = Vec::new();
            let mut k = 0usize;
            while k < name.len() && name[k] != 0 {
                trimmed.push(name[k]);
                k += 1;
            }
            let mut found = None;
            let mut free = None;
            let mut i = 0usize;
            while i < 4 {
                match (&self.names[i], found) {
                    (Some(n), None) if n.as_slice() == trimmed.as_slice() => found = Some(i),
                    _ => {}
                }
                if self.names[i].is_none() && free.is_none() {
                    free = Some(i);
                }
                i += 1;
            }
            match found {
                Some(i) => Some(i),
                None => match free {
                    Some(i) => {
                        self.names[i] = Some(name.to_vec());
                        self.files[i] = Some(Vec::new());
                        Some(i)
                    }
                    None => None,
                },
            }
        }
        fn read(&self, h: usize, off: usize, dst: &mut [u8]) -> usize {
            match self.files.get(h) {
                Some(Some(d)) => {
                    let mut k = 0;
                    while off + k < d.len() && k < dst.len() {
                        dst[k] = d[off + k];
                        k += 1;
                    }
                    k
                }
                _ => 0,
            }
        }
        fn write(&mut self, h: usize, off: usize, src: &[u8]) -> usize {
            match self.files.get_mut(h) {
                Some(Some(d)) => {
                    if d.len() < off + src.len() {
                        d.resize(off + src.len(), 0);
                    }
                    let mut k = 0;
                    while k < src.len() {
                        d[off + k] = src[k];
                        k += 1;
                    }
                    src.len()
                }
                _ => 0,
            }
        }
        fn size(&self, h: usize) -> usize {
            match self.files.get(h) {
                Some(Some(d)) => d.len(),
                _ => 0,
            }
        }
    }

    #[test]
    fn open_read_roundtrip() {
        let mut fs = MemFs::new();
        let h = fs.open(b"init").unwrap();
        fs.files[h] = Some(b"\x7fELF".to_vec());

        let mut req = [0u8; REQ_SIZE];
        build_request(FS_OP_OPEN, 0, 0, 0, b"init", &mut req);
        let mut resp = [0u8; RESP_SIZE];
        serve(&req, &mut resp, &mut fs);
        let handle = resp_status(&resp);
        assert!(handle >= 0);

        build_request(FS_OP_READ, handle as u16, 0, 32, b"", &mut req);
        let n = serve(&req, &mut resp, &mut fs);
        assert_eq!(n, RESP_SIZE);
        assert_eq!(resp_status(&resp), 0);
        assert_eq!(resp_len(&resp), 4);
        assert_eq!(resp_data(&resp, 0), 0x7f);
        assert_eq!(resp_data(&resp, 1), b'E');
    }

    #[test]
    fn stat_and_unknown_op() {
        let mut fs = MemFs::new();
        let h = fs.open(b"x").unwrap();
        fs.files[h] = Some(vec![0u8; 127]);
        let mut req = [0u8; REQ_SIZE];
        build_request(FS_OP_STAT, h as u16, 0, 0, b"", &mut req);
        let mut resp = [0u8; RESP_SIZE];
        serve(&req, &mut resp, &mut fs);
        assert_eq!(resp_status(&resp), 0);
        assert_eq!(resp_len(&resp), 127);

        build_request(99, 0, 0, 0, b"", &mut req);
        serve(&req, &mut resp, &mut fs);
        assert_eq!(resp_status(&resp), -1);
    }

    #[test]
    fn short_request_rejected() {
        let mut fs = MemFs::new();
        let mut resp = [0u8; RESP_SIZE];
        let n = serve(&[0u8; 4], &mut resp, &mut fs);
        assert_eq!(n, RESP_SIZE);
        assert_eq!(resp_status(&resp), -1);
    }

    #[test]
    fn write_then_read_roundtrip() {
        let mut fs = MemFs::new();
        let h = fs.open(b"OUT").unwrap();
        let mut req = [0u8; REQ_DATA_SIZE];
        let mut resp = [0u8; RESP_SIZE];

        build_request_data(FS_OP_WRITE, h as u16, 0, 4, b"", b"VFS!", &mut req);
        serve(&req, &mut resp, &mut fs);
        assert_eq!(resp_status(&resp), 0);
        assert_eq!(resp_len(&resp), 4);

        let mut req28 = [0u8; REQ_SIZE];
        build_request(FS_OP_READ, h as u16, 0, 32, b"", &mut req28);
        serve(&req28, &mut resp, &mut fs);
        assert_eq!(resp_len(&resp), 4);
        assert_eq!(resp_data(&resp, 0), b'V');
        assert_eq!(resp_data(&resp, 1), b'F');
        assert_eq!(resp_data(&resp, 2), b'S');
        assert_eq!(resp_data(&resp, 3), b'!');
    }
}
