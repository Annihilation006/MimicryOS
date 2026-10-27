# test 分支设计与路线（docs/design.md）

> 维护：homo-ai (Hugo_YuGuo)。本文件描述 test 分支的技术事实与演进计划，
> 是对 TEST-BRANCH.md（分工与礼仪）的技术侧补充。

## 1. 总体架构（当前事实）

```
┌──────────────────────────────────────────────┐
│ BOOTBOOT 引导器（保留，main 作者的既有选择）    │
│   UEFI/BIOS 双路、串口已初始化、静态页表        │
├──────────────────────────────────────────────┤
│ C 内核主体（main 分支代码，原样保留）           │
│   控制台/字体、堆、FAT12、PS/2 Shell、模块系统  │
├──────────────────────────────────────────────┤
│ 汇编地基 cpu.asm（test 分支新增）              │
│   端口IO、CLI/STI、CR0-4、CPUID、RDTSC、       │
│   MSR、INVLPG —— 全部 asm_ 前缀               │
├──────────────────────────────────────────────┤
│ Rust 核心（test 分支新增，no_std）             │
│   frame      物理帧位图分配器 + FrameSource   │
│   kmem       帧供给的小对象池                  │
│   vma        虚拟内存区域簿记（链表→BTree）    │
│   pagetable  四级页表 map/unmap/translate     │
├──────────────────────────────────────────────┤
│ （main 作者进行中：IDT/中断入口/上下文切换/     │
│   系统调用 —— test 分支主动绕开，不双线实现）    │
└──────────────────────────────────────────────┘
```

## 2. 模块契约

| 模块 | 依赖 | 内核侧供给 | 宿主测试 |
|---|---|---|---|
| frame | 无 | .bss 位图（4GiB 上限，128KiB） | Vec 背衬，7 项 |
| kmem | frame::FrameSource | rust_kmem_init(frames) 拉帧 | Arena，4 项 |
| vma | 无 | .bss 节点池 ×256 | 栈上池，7 项 |
| pagetable | frame::FrameSource | 当前 CR3（asm_read_cr3） | Arena，6 项 |
| elf | 无 | 静态 ELF64 解析与加载 | 内存缓冲区，2 项 |
| elfprog | elf | 内嵌 ELF 用户镜像构建 | 内存缓冲区，2 项 |
| translator | 无 | 用户态 VFS 翻译器纯逻辑核 serve(FsBack) | MemFs 替身，4 项 |
| 集成 | frame+pagetable+vma | — | test_root.rs，3 项 |

**Ring3 任务表**（调度器容量 12，现用 11）：
0 kernel_main/shell · 1-4 内核线程 · 5 userland · 6 userland2(VMO读数+心跳)
· 7 ELF 任务（tmpfs 装载，退出后槽被回收）· 8 userland3(VFS 翻译器服务)
· 9 userland4(VFS 客户端 + SYS_SPAWN 演示) · 10 userland5(键盘回显)

**IPC 通道表**（NCHAN=3）：0 任务A→ipc服务 · 1 翻译器请求 · 2 翻译器应答。
recv 系统调用按端点令牌路由；空通道登记每通道等待者后阻塞。

**系统调用号**：0 read(键盘队列, 空则阻塞) · 1 画点/write(fd1→行11,
fd2→行14) · 2 字形 · 4 IPC_SEND · 5 IPC_RECV · 6 OPEN · 7 WRF · 8 RDF
· 9 VMO_CREATE · 10 VMO_GETADDR · 11 SPAWN(tmpfs ELF→新任务) · 12 YIELD
· 13 FILL(先擦后画) · 14 SLEEP(tick 定时, 睡眠表 8 项) · 15 BRK(用户态
bump 堆, 16MB 起 4MB) · 60 EXIT(死槽回收)。

**定时**：kernel.c pit_tick 首行调 rust_tick_hook（每 tick）——Rust 自持
RUST_TICKS 并唤醒到点睡眠任务。

**直映射**：rust_direct_map_init 在现行页表空闲 PML4 槽（272）铺 1GB
大页窗口（0xFFFF_8800_0000_0000 起），不切 CR3；DIRECT 标志置位后
frame_to_virt 与 spawn/BRK 的段拷贝走该窗口。rust_vm_bootstrap（切 CR3
版）封存备用。帧分配器 40MB 以下不发帧——6-40MB 是恒等窗口专属区
（SPAWN 装载 / BRK 堆 / 直映射表帧），避免分配帧与窗口内容撞车。

**FrameSource** 是 Rust 侧的供给接口：`alloc_frame/free_frame/frame_to_virt`。
内核实现依赖 BOOTBOOT 的低地址身份映射（phys == virt）；内核已铺自己的
直映射区（rust_direct_map_init），`FrameSource for FrameAllocator` 按
DIRECT 标志自动切换。

**装载窗口表（WINTAB）**：spawn 的段+栈窗口按任务槽分配，SYS_EXIT 时
归还——死槽回收与窗口回收同步闭环，长期运行无泄漏。

## 3. C ↔ Rust 接口清单（extern "C"，C 侧直接声明即用）

```
usize rust_mem_init()                 解析 BOOTBOOT 内存图，返回空闲帧数
u64   rust_frame_alloc()              物理帧号；u64::MAX=失败
i32   rust_frame_free(u64 frame_no)   0=成功
usize rust_kmem_init(u64 frames)      给 Rust 对象池补帧
i32   rust_vma_init()                 初始化 VMA（内核竞技场 8TiB）
u64   rust_vma_mmap(u64 len, u64 flags)     返回 vaddr；u64::MAX=失败
i32   rust_vma_munmap(u64 start, u64 len)   0=有变化
i32   rust_map_page(u64 vaddr, u64 paddr, u64 flags)   作用于当前 CR3
i32   rust_unmap_page(u64 vaddr)
void  rust_mem_report()               fb_print 输出统计
u64   rust_cpu_report()               返回 CR0（顺带打印厂商串）
```

页表 flags 位（传给 rust_map_page）：WRITABLE=1<<1，USER=1<<2，
NO_EXEC=1<<63（PRESENT 自动置位）。

## 4. 调度与键盘（现状事实）

- **优先级**：Task.prio 数小者优先；schedule 严格更高独占 CPU，同级
  轮转。当前全部默认 1（内核线程仍是自旋体，提级会饿死用户任务——
  线程改 sleep 后再接线）
- **键盘**：kbdrv.asm（纯汇编 IRQ1 驱动）＝33 号门 + 扫描码翻译 +
  双环形队列（A=shell 阻塞 getc，B=SYS_READ 非阻塞 take）。初始化
  时先排干 8042 输出缓冲再改配置字节（bit0 中断使能）——不排干会把
  BIOS 残留的 ACK 当配置读，中断永远不触发。自证静态
  kb_cfg_read/kb_cfg_write 可 gdb 查看

## 5. 质量基建

- **单测**：每个模块内嵌 `#[cfg(test)]`；`rustc --test tools/test_root.rs`
  一把全跑（宿主执行，不需要真机）
- **CI**：`bash tools/ci.sh` = 单测 → 混合构建 → mkbootimg →
  QEMU 无头启动 → 截屏亮度检查，五段流水线
- **启动配方**：mkbootimg 需要 `boot/kernel.elf` + `boot/sys/config`
  （`kernel=kernel.elf`），ci.sh 已内置

## 6. 路线（2024 修订：中断/调度/系统调用已由 test 分支实现）

1. ✅ 物理帧位图分配器（保守放开策略）
2. ✅ 小对象池 / VMA 簿记 / 四级页表操作
3. ✅ 直映射区（原位铺窗口版）；⬜ 每进程页表（vmboot 切 CR3 版
   三重故障待查——已知线索：分配器可能发到 BOOTBOOT 自身页表所在帧）
4. ⬜ 缺页处理路径
5. ⬜ fork 的深拷贝 + COW 引用计数
6. ✅ 用户态地址空间模板（aspace::new_user，待 CR3 切换解锁）
