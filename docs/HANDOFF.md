# MimicryOS — test 分支交付说明

写给接手的朋友：这个分支在你的 main 之上**只做加法**——你原有的 C 代码
（kernel.c/idt/interrupt/pic/console/isr/fat12/模块系统）一行没删，所有新
设施都挂在 `rust_kernel.rs` 为根的 Rust 侧，通过 4 个挂点接进你的内核。

## 快速上手

```bash
bash tools/ci.sh        # 单测 + 混合构建 + QEMU 无头启动 + 屏幕检查, 约 1 分钟
```

启动后屏幕从上到下（每行证明一条链路）：

| 屏幕行 | 内容 | 证明的链路 |
|---|---|---|
| 右上角 | tick 计数 | PIT 100Hz 中断活着 |
| 第 4-5 行 | 双线程计数 | Rust 内核线程被调度 |
| 第 8 行 | `V012 N023` | VMO 共享内存：A 写、B 读、心跳 |
| 第 9 行 | 计数 | LOG 文件写→内核读 |
| 第 11 行 | `ELF OK` 绿字 | tmpfs 文件 → ELF 装载 → ring3 进程 |
| 第 12 行 | `VFS 004 OK` | 用户态 VFS 翻译器 RPC 全链路 |
| 第 13 行 | 你敲的键 | 键盘 → SYS_READ 阻塞读 → 回显 |
| 第 14 行 | `SPAWN OK` | SYS_SPAWN 装载的进程自己画的 |
| 第 15 行 | `S007` | 死槽回收：复用了退出的 ELF 任务槽 |
| 第 16 行 | `B OK` | SYS_BRK 用户态堆分配 |

shell 可用命令（新增）：`run <file> [arg]` —— 把 tmpfs 里的 ELF 装载为新
任务；`help` 查看全部。

## 架构一句话

微内核：调度/IPC/能力/内存管理在内核环，文件系统和驱动意图上全部走
用户态翻译器（当前翻译器背后暂挂内核 tmpfs）。

## 代码地图（全部在仓库根，Rust，no_std）

- `rust_kernel.rs` — Rust 侧根：系统调用分发、任务初始化、输入队列、
  装载窗口表、键盘队列、直映射初始化
- `task.rs` — 调度核：优先级抢占 + 同级轮转、死槽回收、睡眠表
- `ipc.rs` — 通道/消息/VMO；`capability.rs` — CSpace
- `frame/kmem/vma/pagetable/vmboot/aspace.rs` — 内存管理族
- `tmpfs.rs` — 内存文件系统；`translator.rs` — VFS 翻译器纯逻辑核
- `elf.rs`/`elfprog.rs` — ELF 解析装载 / 内嵌镜像构建
- `syscall.rs` — 调用号、帧布局、微字库、渲染后端抽象
- `userland*.rs` — 5 个 ring3 任务
- `vmo.asm`/`usermode.asm` — 共享内存池、ring3 跳板（asm 只做机制）

## 对 C 侧的全部改动（共 5 处，全是加法）

1. `kernel.c` 头部：3 个 extern 声明
2. `kernel.c` `kernel_main`：调 `rust_direct_map_init` / `rust_tasks_init`
3. `kernel.c` `shell()`：`rust_kb_feed` 一行 + `run` 命令分支
4. `kernel.c` `pit_tick`：`rust_tick_hook()` 一行（每 tick 睡眠唤醒）

## 系统调用一览

`0` read(键盘) · `1` write/画点 · `2` 字形 · `4/5` IPC 收发 · `6/7/8`
open/write/read · `9/10` VMO · `11` spawn · `12` yield · `13` fill ·
`14` sleep · `15` brk · `60` exit。能力门控所有破坏性操作。

## 已知边界（想继续从这里下手）

- argv 传参：内核侧栈块已验证，用户态读参镜像待调（`elfprog::build_image_arg`）
- 能力槽无回收：`run` 反复执行会耗尽 CSpace 槽位，需加 revoke
- 每进程页表：`vmboot::build`（切 CR3 版）封存，QEMU 下三重故障待查
- 优先级：机制就绪（`set_prio`），等内核线程改睡眠后接线
