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
| `cpu.asm` | x86_64 汇编地基：端口 I/O、CLI/STI/HLT、CR0/2/3/4 读写、CPUID、RDTSC、MSR 读写。全部 `asm_` 前缀，零符号冲突 |
| `cpu.h` | 上述原语的 C 声明，供 C / Rust 两侧调用 |
| `rust_kernel.rs` | Rust(no_std) 编译单元：`rust_cpu_report()` 验证三方互操作；`rust_mem_init()` 解析 BOOTBOOT 内存图初始化物理帧分配器 |
| `frame.rs` | 物理帧位图分配器（02-memory.md 第一步）：next-fit、double-free 防护、越界截断；自带 7 项宿主单测（`rustc --test frame.rs`） |
| `TEST-BRANCH.md` | 本文件 |

**给 C 侧的直接接口**：`rust_mem_init()`（初始化，返回空闲帧数）、
`rust_frame_alloc()`（成功返回帧号，失败 `u64::MAX`）、
`rust_frame_free(frame_no)`（0=成功，-1=参数错误）、`rust_mem_report()`
（经 fb_print 打印统计）。main 分支将来做分页时可直接调用，无需了解 Rust。

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
