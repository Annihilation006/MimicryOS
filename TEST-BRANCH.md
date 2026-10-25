# test 分支说明

> 写给每一位（尤其是 @Annihilation006）翻到这个分支的人。
> 维护者：homo-ai (Hugo_YuGuo)

## 这个分支是什么

`test` 是我的实验分支：main 上的代码与节奏**完全不动、完全尊重**，
我在这里按自己的路线搭下一层东西。合不合、何时合，都由 main 的主人说了算。

## 分工原则（求同存异）

- **已经写好的部分以原作者为准**：main 上的引导、控制台、堆、文件系统、
  键盘 Shell 等模块在本分支一律不改动、不重写，只调用、只共存。
- **同一方向正在写的部分也以原作者为准**：IDT / 中断入口 / 上下文切换 /
  系统调用这些正在推进的方向，本分支**主动绕开**，不搞双线实现。
- **尚未开工的部分由本分支推进**：从最底层的 CPU 原语开始，
  之后的新代码以 Rust 为主（C 侧通过 `extern "C"` 互操作）。

## 目前新增了什么

| 文件 | 说明 |
|---|---|
| `cpu.asm` / `cpu.h` | x86_64 汇编地基：端口 I/O、CLI/STI/HLT、CR0/2/3/4、CPUID、RDTSC、MSR、INVLPG。全部 `asm_` 前缀，零符号冲突 |
| `rust_kernel.rs` | Rust(no_std) 编译单元与模块装配：CPU 桥、BOOTBOOT 内存图解析、内核侧 statics 与 C 接口 |
| `frame.rs` | 物理帧位图分配器 + FrameSource 供给接口（保守放开策略） |
| `kmem.rs` | 帧供给的内核小对象池（K&R 空闲链表，相邻合并） |
| `vma.rs` | 虚拟内存区域簿记（有序链表，mmap/munmap 语义，256 节点池） |
| `pagetable.rs` | x86_64 四级页表 map/unmap/translate（作用于当前 CR3） |
| `tools/test_root.rs` | 宿主测试入口：`rustc --edition 2021 --test` 一把跑全部 26 项单测 |
| `tools/ci.sh` | 五段 CI：单测 → 混合构建 → 打包 → QEMU 启动 → 屏幕点亮检查 |
| `docs/design.md` | 技术事实与路线图（架构图、模块契约、C 接口清单） |
| `idt.h` / `idt.c` | 中断描述符表（原 32 位版在长模式必崩，按原作者骨架修正为 16 字节门） |
| `isr.asm` | 48 个中断桩 + 公共入口 + idt_load（补完 WIP 里缺失的部分） |
| `interrupt.c` / `pic.c` / `pic.h` | 异常/IRQ 分发器与 8259A 重映射（原作者 WIP 原样移植） |
| `console.c` | print/print_hex 薄壳（原版直写 0xB8000 在 BOOTBOOT 图形模式不存在，改走 fb_print） |
| `TEST-BRANCH.md` | 本文件 |

**中断现状**：IDT + PIC 重映射 + PIT 100Hz 已接线 kernel_main，QEMU 实测
IRQ0 持续触发（右上角绿色 tick 计数器跳动）。原作者的 WIP 快照在
`wip-interrupt` 分支（fab890e），本分支的中断实现以其骨架为底补完。

**给 C 侧的直接接口**（完整清单与 flags 位定义见 `docs/design.md`）：
`rust_mem_init / rust_frame_alloc / rust_frame_free / rust_kmem_init /
rust_vma_init / rust_vma_mmap / rust_vma_munmap / rust_map_page /
rust_unmap_page / rust_mem_report / rust_cpu_report`。

构建说明：`make` 即可。装有 rustup（目标 `x86_64-unknown-none`）的机器会
自动编入 Rust 对象；没有 rustc 的机器会打印提示并跳过，C 侧构建不受影响。

**本地启动验证配方**（main 的 README 里缺了 boot 目录组装这步，在此补全）：

```
mkdir -p boot/sys
cp kernel.elf boot/kernel.elf          # kernel 放 boot/ 根下
printf 'kernel=kernel.elf\n' > boot/sys/config
<path-to>/mkbootimg iso_build/myos.json iso_build/myos.img
qemu-system-x86_64 -hda iso_build/myos.img -m 1024M
```

后续计划：VMA 管理、页表搭建（复用 `asm_read_cr3/asm_write_cr3`）。
每一步都保持"不动已有代码、只做加法"；`rust_*()` 系列何时接线调用，
等 main 作者点头。

## 署名约定

本分支的提交以提交者真实身份署名（homo-ai / Hugo_YuGuo）。
也借此提个小小的请求：main 上如果合入了来自他人的代码，
哪怕在 README 或提交信息里提一句来源，对贡献者是很重要的事。😊
