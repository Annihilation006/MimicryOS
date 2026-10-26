//! 用户态 VFS 翻译器服务框架（01/06-vfs.md）
//! 纯 Rust、no_std、RPC over IPC 通道协议

pub const FS_OP_OPEN: u16 = 1;
pub const FS_OP_READ: u16 = 2;
pub const FS_OP_WRITE: u16 = 3;
pub const FS_OP_STAT: u16 = 4;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FsRequest {
    pub op: u16,
    pub fd: u16,
    pub offset: u32,
    pub len: u32,
    pub name: [u8; 16],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct FsResponse {
    pub status: i32,
    pub len: u32,
    pub data: [u8; 32],
}

pub struct VfsTranslator;

impl VfsTranslator {
    pub fn handle_request(req_bytes: &[u8], resp_bytes: &mut [u8]) -> usize {
        let req_size = core::mem::size_of::<FsRequest>();
        let resp_size = core::mem::size_of::<FsResponse>();

        if req_bytes.len() < req_size || resp_bytes.len() < resp_size {
            return 0;
        }

        let req = unsafe { &*(req_bytes.as_ptr() as *const FsRequest) };
        let resp = unsafe { &mut *(resp_bytes.as_mut_ptr() as *mut FsResponse) };

        match req.op {
            FS_OP_OPEN => {
                resp.status = 0; // 成功模拟打开
                resp.len = 0;
            }
            FS_OP_READ => {
                resp.status = 0;
                resp.len = 4;
                resp.data[0..4].copy_from_slice(b"VFS\n");
            }
            FS_OP_WRITE => {
                resp.status = 0;
                resp.len = req.len;
            }
            _ => {
                resp.status = -1; // 不支持的操作
                resp.len = 0;
            }
        }
        resp_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vfs_translator_open_and_read() {
        let mut req = FsRequest {
            op: FS_OP_OPEN,
            fd: 0,
            offset: 0,
            len: 0,
            name: [0; 16],
        };
        req.name[0..4].copy_from_slice(b"TEST");

        let mut req_buf = [0u8; 64];
        unsafe {
            core::ptr::copy_nonoverlapping(
                &req as *const _ as *const u8,
                req_buf.as_mut_ptr(),
                core::mem::size_of::<FsRequest>(),
            );
        }

        let mut resp_buf = [0u8; 64];
        let n = VfsTranslator::handle_request(&req_buf, &mut resp_buf);
        assert_eq!(n, core::mem::size_of::<FsResponse>());
        let resp = unsafe { &*(resp_buf.as_ptr() as *const FsResponse) };
        assert_eq!({ resp.status }, 0);

        // Read 测试
        req.op = FS_OP_READ;
        unsafe {
            core::ptr::copy_nonoverlapping(
                &req as *const _ as *const u8,
                req_buf.as_mut_ptr(),
                core::mem::size_of::<FsRequest>(),
            );
        }
        let _ = VfsTranslator::handle_request(&req_buf, &mut resp_buf);
        let resp_read = unsafe { &*(resp_buf.as_ptr() as *const FsResponse) };
        assert_eq!({ resp_read.status }, 0);
        assert_eq!(&resp_read.data[0..4], b"VFS\n");
    }
}
