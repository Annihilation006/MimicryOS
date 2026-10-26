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
    pub dead: bool,   // 已退出: 槽位可回收; 阻塞的任务只是 live=false
    pub prio: u8,     // 优先级, 数小者优先, 默认 1
    pub irq_top: u64, // 该任务的内核中断栈顶 (TSS.RSP0), 0 = 未分配
}

// 定时睡眠表项: 到点被 tick 钩子唤醒
#[derive(Clone, Copy)]
pub struct Sleeper {
    pub task: usize,
    pub deadline: u64,
    pub used: bool,
}

pub const NSLEEP: usize = 8;

pub struct Scheduler {
    pub t: [Task; 12],
    pub n: usize,   // 已占槽上限（死槽可复用）
    pub cur: usize, // 当前运行槽
    pub sleepers: [Sleeper; NSLEEP],
}

impl Scheduler {
    pub const fn new() -> Self {
        Scheduler {
            t: [Task { rsp: 0, live: false, dead: false, prio: 1, irq_top: 0 }; 12],
            n: 0,
            cur: 0,
            sleepers: [Sleeper { task: 0, deadline: 0, used: false }; NSLEEP],
        }
    }

    // 把正在跑的上下文（kernel_main/shell）收编为任务 0:
    // rsp=0 表示还没被打断过, 第一次 tick 自然把真现场存进来
    pub unsafe fn adopt_current(&mut self) -> usize {
        if self.n >= 12 {
            return usize::MAX;
        }
        let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
        *t.add(self.n) = Task { rsp: 0, live: true, dead: false, prio: 1, irq_top: 0 };
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

    // 调优先级: 数小者优先
    pub unsafe fn set_prio(&mut self, idx: usize, prio: u8) {
        if idx < self.n {
            let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
            (*t.add(idx)).prio = prio;
        }
    }

    // stack_top 必须页对齐; 入口永远不返回
    pub unsafe fn spawn(&mut self, entry: u64, stack_top: u64) -> Option<usize> {
        self.spawn_with(entry, stack_top, KCODE, KDATA)
    }

    // 每 tick 被调: frame=当前现场位置。返回下一个该跑的 rsp, 单任务原样返回
    // 别用 %: 运行时取余会生成除零 panic 桩, 裸机链不上
    // 优先级: 数小者优先; 同级轮转。当前任务已是最高优先级就原样返回不切
    pub unsafe fn schedule(&mut self, frame: u64) -> u64 {
        if self.n <= 1 {
            return frame;
        }
        let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
        (*t.add(self.cur)).rsp = frame;
        // 找最高优先级 (数最小) 的活任务; 同级取 cur 之后最近的
        let mut best = usize::MAX;
        let mut best_prio = 255u8;
        let mut i = self.cur;
        let mut steps = 0usize;
        while steps < self.n {
            i += 1;
            if i >= self.n {
                i = 0;
            }
            steps += 1;
            let task = &*t.add(i);
            if task.live && task.prio < best_prio {
                best_prio = task.prio;
                best = i;
            }
        }
        if best == usize::MAX {
            return frame; // 没有别的活任务
        }
        let cur_task = &*t.add(self.cur);
        if cur_task.live && cur_task.prio < best_prio {
            return frame; // 当前任务严格更高: 独占; 同级照常轮转
        }
        self.cur = best;
        // 切到 ring3 任务前换它自己的中断栈, 不然大家的帧在 RSP0 上互相踩
        // (宿主测试里没有 tss 符号, cfg 隔离)
        #[cfg(not(test))]
        {
            let top = (*t.add(best)).irq_top;
            if top != 0 {
                extern "C" {
                    static tss: u8;
                }
                core::ptr::write_volatile((&raw const tss as u64 + 4) as *mut u64, top);
            }
        }
        #[cfg(test)]
        let _ = (*t.add(best)).irq_top;
        (*t.add(best)).rsp
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

    // 退出当前任务: 与阻塞不同, 槽位可回收, wake 救不回来
    pub unsafe fn exit_cur(&mut self) {
        let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
        (*t.add(self.cur)).live = false;
        (*t.add(self.cur)).dead = true;
    }

    pub unsafe fn wake(&mut self, idx: usize) -> bool {
        if idx < self.n {
            let t = core::ptr::addr_of_mut!(self.t) as *mut Task;
            let task = &mut *t.add(idx);
            if !task.dead {
                task.live = true;
                return true;
            }
        }
        false
    }

    // 登记定时睡眠: 满/重复登记返回 false
    pub fn sleep_push(&mut self, task: usize, deadline: u64) -> bool {
        for i in 0..NSLEEP {
            if self.sleepers[i].used && self.sleepers[i].task == task {
                return false;
            }
        }
        for i in 0..NSLEEP {
            if !self.sleepers[i].used {
                self.sleepers[i] = Sleeper { task, deadline, used: true };
                return true;
            }
        }
        false
    }

    // tick 钩子: 唤醒所有到点任务, 返回真唤醒数 (退出任务不救)
    pub fn sleep_wake(&mut self, now: u64) -> usize {
        let mut woke = 0usize;
        for i in 0..NSLEEP {
            if self.sleepers[i].used && now >= self.sleepers[i].deadline {
                let t = self.sleepers[i].task;
                self.sleepers[i] = Sleeper { task: 0, deadline: 0, used: false };
                if unsafe { self.wake(t) } {
                    woke += 1;
                }
            }
        }
        woke
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
        // 死槽复用: 退出任务的槽位给新任务, irq_top 沿用原槽的
        let mut i = 0usize;
        while i < self.n {
            if (*t.add(i)).dead {
                *t.add(i) = Task { rsp: frame, live: true, dead: false, prio: 1, irq_top: (*t.add(i)).irq_top };
                return Some(i);
            }
            i += 1;
        }
        *t.add(self.n) = Task { rsp: frame, live: true, dead: false, prio: 1, irq_top: 0 };
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

    #[test]
    fn t7_exit_slot_recycled() {
        let (_s0, t0) = mkstack();
        let (_s1, t1) = mkstack();
        let mut s = Scheduler::new();
        unsafe {
            s.spawn(0x1000, t0);
            s.spawn(0x2000, t1);
            s.exit_cur(); // 0 号退出
            // wake 救不回退出的任务
            s.wake(0);
            assert_eq!(s.schedule(0x60_0000), t1 - 176);
            // 退出槽位被新任务复用: 槽 0, irq_top 沿用
            let slot = s.spawn_user(0xBEEF, t0);
            assert_eq!(slot, Some(0));
            // 轮转应调度到复用槽
            assert_eq!(s.schedule(0x61_0000), t0 - 176);
        }
    }

    #[test]
    fn t8_sleep_wake_on_deadline() {
        let mut stacks = Vec::new();
        let mut s = Scheduler::new();
        unsafe {
            for _ in 0..8 {
                let (v, top) = mkstack();
                stacks.push(v);
                s.spawn(0x1000, top);
            }
        }
        // 登记睡眠 100 ticks, 未到点不唤醒
        assert!(s.sleep_push(0, 100));
        assert_eq!(s.sleep_wake(50), 0);
        assert_eq!(s.sleep_wake(99), 0);
        // 到点唤醒, 表项回收
        assert_eq!(s.sleep_wake(100), 1);
        assert_eq!(s.sleep_wake(200), 0);
        // 重复登记拒绝
        assert!(s.sleep_push(0, 300));
        assert!(!s.sleep_push(0, 400));
        // 表满拒绝
        for t in 1..NSLEEP {
            assert!(s.sleep_push(t, 1000));
        }
        assert!(!s.sleep_push(9, 1000));
        // 退出任务的睡眠项到点也不该把它救活 (wake 忽略 dead)
        unsafe { s.exit_cur() };
        assert_eq!(s.sleep_wake(1000), NSLEEP - 1);
    }

    #[test]
    fn t9_priority_preempts() {
        let (_s0, t0) = mkstack();
        let (_s1, t1) = mkstack();
        let (_s2, t2) = mkstack();
        let mut s = Scheduler::new();
        unsafe {
            s.spawn(0x1000, t0);
            s.spawn(0x2000, t1);
            s.spawn(0x3000, t2);
        }
        // 2 号提为高优先级 (数小): 无论轮到谁, tick 一来就切到 2 号
        unsafe { s.set_prio(2, 0); }
        unsafe {
            assert_eq!(s.schedule(0x50_0000), t2 - 176);
            // 2 号再次被打断: 存的是新现场, 恢复也用新现场
            assert_eq!(s.schedule(0x51_0000), 0x51_0000);
        }
        // 高优先级阻塞后回落到普通轮转 (0 号的保存现场是它上次被打断处)
        unsafe { s.block_cur() };
        unsafe { s.set_prio(2, 1); }
        unsafe {
            assert_eq!(s.schedule(0x52_0000), 0x50_0000);
        }
    }

    #[test]
    fn t10_same_prio_round_robins() {
        let (_s0, t0) = mkstack();
        let (_s1, t1) = mkstack();
        let mut s = Scheduler::new();
        unsafe {
            s.spawn(0x1000, t0);
            s.spawn(0x2000, t1);
        }
        // 同级不打压: 依然轮转; 恢复现场 = 各自上次被打断处
        unsafe {
            assert_eq!(s.schedule(0x50_0000), t1 - 176);
            assert_eq!(s.schedule(0x51_0000), 0x50_0000);
            assert_eq!(s.schedule(0x52_0000), 0x51_0000);
        }
    }
}
