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
| 集成 | frame+pagetable+vma | — | test_root.rs，2 项 |

**FrameSource** 是 Rust 侧的供给接口：`alloc_frame/free_frame/frame_to_virt`。
内核实现依赖 BOOTBOOT 的低地址身份映射（phys == virt）；一旦内核建立
自己的直映射区，只需改 `FrameSource for FrameAllocator` 一处。

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

## 4. 质量基建

- **单测**：每个模块内嵌 `#[cfg(test)]`；`rustc --test tools/test_root.rs`
  一把全跑（宿主执行，不需要真机）
- **CI**：`bash tools/ci.sh` = 单测 → 混合构建 → mkbootimg →
  QEMU 无头启动 → 截屏亮度检查，五段流水线
- **启动配方**：mkbootimg 需要 `boot/kernel.elf` + `boot/sys/config`
  （`kernel=kernel.elf`），ci.sh 已内置

## 5. 路线（按 02-memory.md 之后的顺序）

1. ✅ 物理帧位图分配器（保守放开策略）
2. ✅ 小对象池 / VMA 簿记 / 四级页表操作
3. ⬜ 页表自举：建立内核直映射区，替换身份映射假设
4. ⬜ 缺页处理路径（需与 main 作者的中断设施衔接）
5. ⬜ fork 的深拷贝 + COW 引用计数
6. ⬜ 用户态地址空间模板（Ring 3 就绪后）

中断/调度/系统调用不在此路线内——那是 main 作者的领地（求同存异）。
