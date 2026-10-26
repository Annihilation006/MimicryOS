//! 抢占式调度核（03-process 第一步）：轮转 + 优先级留槽
//! 机关：中断桩把完整现场压在当前任务栈上, handler 返回新 rsp, 桩换栈弹寄存器 iretq
//! 帧布局 22*8=176 字节, 与 isr.asm 的压栈顺序强耦合, 别乱动

pub const FRAME_SLOTS: usize = 22; // 15 gpr + int_no + err + rip/cs/rflags/rsp/ss
pub const KCODE: u64 = 0x08;
pub const KDATA: u64 = 0x10;
pub const UCODE: u64 = 0x1B; // 0x18|3 — iretq 进 ring3 必须 RPL=3
pub const UDATA: u64 = 0x23; // 0x20|3

#[derive(Clone, Copy)]
pub struct Task {
    pub rsp: u64, // 指向 gpr 块; 0 = 还没被打断过
    pub live: bool,
    pub irq_top: u64, // 该任务的内核中断栈顶 (TSS.RSP0), 0 = 未分配
}

pub struct Scheduler {
    pub t: [Task; 12],
    pub n: usize,   // 已占槽（密集分配, 不回收）
    pub cur: usize, // 当前运行槽
}

impl Scheduler {
    pub const fn new() -> Self {
        Scheduler {
            t: [Task { rsp: 0, live: false, irq_top: 0 }; 12],
            n: 0,
            cur: 0,
        }
    }

    // 把正在跑的上下文（kernel_main/shell）收编为任务 0:
    // rsp=0 表示还没被打断过, 第一次 tick 自然把真现场存进来
    pub unsafe fn adopt_current(&mut self) -> usize {
        if self.n >= 12 {
            return usize::MAX;
        }
        let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
        *t.add(self.n) = Task { rsp: 0, live: true, irq_top: 0 };
        self.n += 1;
        self.cur = 0;
        self.n - 1
    }

    // 每任务独立中断栈顶写入 TSS.RSP0 — ring3 任务共用一条 RSP0 会互相踩帧
    pub unsafe fn set_irq_top(&mut self, idx: usize, top: u64) {
        if idx < self.n {
            let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
            (*t.add(idx)).irq_top = top;
        }
    }

    // stack_top 必须页对齐; 入口永远不返回
    pub unsafe fn spawn(&mut self, entry: u64, stack_top: u64) -> Option<usize> {
        self.spawn_with(entry, stack_top, KCODE, KDATA)
    }

    // 每 tick 被调: frame=当前现场位置。返回下一个该跑的 rsp, 单任务原样返回
    // 别用 %: 运行时取余会生成除零 panic 桩, 裸机链不上
    pub unsafe fn schedule(&mut self, frame: u64) -> u64 {
        if self.n <= 1 {
            return frame;
        }
        let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
        (*t.add(self.cur)).rsp = frame;
        let mut i = self.cur + 1;
        if i >= self.n {
            i = 0;
        }
        while !(*t.add(i)).live {
            i += 1;
            if i >= self.n {
                i = 0;
            }
        }
        self.cur = i;
        // 切到 ring3 任务前换它自己的中断栈, 不然大家的帧在 RSP0 上互相踩
        // (宿主测试里没有 tss 符号, cfg 隔离)
        #[cfg(not(test))]
        {
            let top = (*t.add(i)).irq_top;
            if top != 0 {
                extern "C" {
                    static tss: u8;
                }
                core::ptr::write_volatile((&raw const tss as u64 + 4) as *mut u64, top);
            }
        }
        #[cfg(test)]
        let _ = (*t.add(i)).irq_top;
        (*t.add(i)).rsp
    }

    // ring3 任务: 帧带用户段选择子, iretq 直接落用户态
    pub unsafe fn spawn_user(&mut self, entry: u64, stack_top: u64) -> Option<usize> {
        self.spawn_with(entry, stack_top, UCODE, UDATA)
    }

    pub fn cur_index(&self) -> usize {
        self.cur
    }

    // 阻塞当前任务: 调度器跳过它, 直到 wake
    pub unsafe fn block_cur(&mut self) {
        let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
        (*t.add(self.cur)).live = false;
    }

    pub unsafe fn wake(&mut self, idx: usize) {
        if idx < self.n {
            let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
            (*t.add(idx)).live = true;
        }
    }

    unsafe fn spawn_with(
        &mut self,
        entry: u64,
        stack_top: u64,
        cs: u64,
        ss: u64,
    ) -> Option<usize> {
        if self.n >= 12 {
            return None;
        }
        let frame = stack_top - (FRAME_SLOTS * 8) as u64;
        let p = frame as *mut u64;
        for i in 0..17 {
            *p.add(i) = 0;
        }
        *p.add(17) = entry;
        *p.add(18) = cs;
        *p.add(19) = 0x202;
        *p.add(20) = stack_top - 8;
        *p.add(21) = ss;
        let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
        *t.add(self.n) = Task { rsp: frame, live: true, irq_top: 0 };
        self.n += 1;
        Some(self.n - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mkstack() -> (Vec<u8>, u64) {
        let mut v = vec![0u8; 4096];
        let top = v.as_mut_ptr() as u64 + 4096;
        (v, top)
    }

    #[test]
    fn t1_frame_layout() {
        let (s0, top) = mkstack();
        let mut s = Scheduler::new();
        let slot = unsafe { s.spawn(0xDEAD, top) }.unwrap();
        assert_eq!(slot, 0);
        let _ = s0;
        let p = (top - 176) as *const u64;
        unsafe {
            assert_eq!(*p.add(17), 0xDEAD);
            assert_eq!(*p.add(18), KCODE);
            assert_eq!(*p.add(19), 0x202);
            assert_eq!(*p.add(20), top - 8);
            assert_eq!(*p.add(21), KDATA);
            assert_eq!(*p.add(0), 0);
        }
    }

    #[test]
    fn t2_round_robin() {
        let (_s0, t0) = mkstack();
        let (_s1, t1) = mkstack();
        let (_s2, t2) = mkstack();
        let mut s = Scheduler::new();
        unsafe {
            s.spawn(0x1000, t0);
            s.spawn(0x2000, t1);
            s.spawn(0x3000, t2);
        }
        let f0 = 0x50_0000u64;
        unsafe {
            assert_eq!(s.schedule(f0), t1 - 176);
            assert_eq!(s.schedule(0x51_0000), t2 - 176);
            assert_eq!(s.schedule(0x52_0000), f0);
        }
    }

    #[test]
    fn t3_single_task_no_switch() {
        let (_s0, t0) = mkstack();
        let mut s = Scheduler::new();
        unsafe { s.spawn(0x1000, t0); }
        unsafe { assert_eq!(s.schedule(0x60_0000), 0x60_0000); }
    }

    #[test]
    fn t4_slots_cap() {
        let mut stacks = Vec::new();
        let mut s = Scheduler::new();
        for _ in 0..12 {
            let (v, top) = mkstack();
            stacks.push(v);
            assert!(unsafe { s.spawn(0x1000, top) }.is_some());
        }
        unsafe { assert_eq!(s.spawn(0x1000, 4096), None); }
    }

    #[test]
    fn t5_user_frame_selectors() {
        let (_s0, top) = mkstack();
        let mut s = Scheduler::new();
        unsafe { s.spawn_user(0xBEEF, top) }.unwrap();
        let p = (top - 176) as *const u64;
        unsafe {
            assert_eq!(*p.add(17), 0xBEEF);
            assert_eq!(*p.add(18), UCODE);
            assert_eq!(*p.add(21), UDATA);
        }
    }

    #[test]
    fn t6_block_and_wake() {
        let (_s0, t0) = mkstack();
        let (_s1, t1) = mkstack();
        let mut s = Scheduler::new();
        unsafe {
            s.spawn(0x1000, t0);
            s.spawn(0x2000, t1);
            s.block_cur(); // 阻塞 0 号
            assert_eq!(s.schedule(0x60_0000), t1 - 176);
            // 只剩 1 号活着: 自己切给自己
            assert_eq!(s.schedule(0x61_0000), 0x61_0000);
            s.wake(0);
            // 唤醒后回到 0 号首次被打断的现场
            assert_eq!(s.schedule(0x62_0000), 0x60_0000);
        }
    }
}
