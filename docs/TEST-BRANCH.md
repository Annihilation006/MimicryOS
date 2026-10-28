# MimicryOS — test 分支说明（Rust 微内核线）

朋友你好。这条分支是在你的内核（C/asm，BOOTBOOT 引导）之上，
用**纯 Rust** 铺的微内核层：能力、IPC、抢占调度、Ring3 用户态、
文件系统、ELF 装载。你的 main 分支代码一行没动，只在 kernel.c 挂了
四个钩子（共 6 行），全部带 `[test]` 注释可搜。

## 分工原则（不变）

- main 上**已写好的**模块：引导、控制台、堆、文件系统、Shell——一律
  不重写，只调用、只共存；对本分支必须触碰的少数几行，全部带
  `[test]` 注释、全部是加法或等义替换（全程累计只替换过 3 行）。
- 同方向**正在写**的部分以原作者为准，不搞双线实现。
- **尚未开工**的部分由本分支推进；语言不限（Rust/C/汇编皆可），
  以与既有模块共存、不破坏原作者结构为准。

> 本文件是唯一的分支状态文档（早期根目录同名说明已并入此处）；
> 模块契约与内存布局见 `docs/design.md`；构建问题见根目录 `BUILD.md`。

## 一分钟看懂

开机后屏幕是活的系统状态面板，每一行证明一条子系统链路：

| 屏幕位置 | 内容 | 证明的链路 |
|---|---|---|
| 右上角 | 三位数跳动 | PIT 100Hz 中断活着 |
| 行 6/7 | 两计数器 | 内核线程 + 抢占调度 |
| 行 8 左 | `V:012` 递增 | 任务A→共享内存(VMO)→任务B 零拷贝读 |
| 行 8 右 | `N:023` 递增 | 任务B 调度心跳（变红=异常指示灯）|
| 行 9 | 白色数字 | 用户写文件→内核存→内核读 全链路 |
| 行 11 | 绿色 `ELF OK` | tmpfs 装载的 ELF 程序在 ring3 跑 |
| 行 12 | 青色 `VFS 004 OK` | 用户态 VFS 翻译器 RPC（IPC 往返）|
| 行 13 | 青色字符 | 键盘→shell→队列→ring3 回显（打字试试）|
| 行 14 | 绿色 `SPAWN OK` | 被 spawn 的进程自己画的 |
| 行 15 | `S:007` | 运行时 spawn 的槽号（死槽复用）|
| 行 16 | `B OK` | 用户态堆分配-写-读回 |

## shell 命令

原有命令全部保留，新增：

```
run <file> [arg]    把 tmpfs 里的 ELF 文件装载为新任务, 打印槽号
```

## 架构（test 分支新增，全部 no_std Rust）

```
kernel.c (你的) ──3 个钩子──┬─ rust_tick_hook       每 tick 睡眠唤醒
                            ├─ rust_direct_map_init 直映射窗口
                            └─ rust_tasks_init      启动整个 Rust 侧

kbdrv.asm = IRQ1 中断驱动键盘 (纯汇编: 33号门 + 扫描码翻译 + 双环形
队列), shell 与 SYS_READ 各占一条队列; 早期 rust_kb_feed 副本机制已由它取代

rust_kernel.rs = 调度(12槽/优先级/睡眠/死槽回收) + 16 个系统调用分发
frame/kmem/vma/pagetable = 内存全套
capability  = 令牌发放校验（帧缓冲/端点/文件）
ipc         = 3 通道环形缓冲, 按端点路由, 阻塞唤醒
tmpfs       = 内存文件系统 (8文件×256B)
elf/elfprog = ELF64 解析 + 位置无关镜像 + 运行时装载
translator  = 用户态 VFS 翻译器协议 (OPEN/READ/WRITE/STAT)
userland1-5 = 六个 ring3 任务 (见上表)
```

系统调用号：0 read · 1 write/画点 · 2 字形 · 4/5 IPC · 6/7/8 文件
· 9/10 VMO · 11 spawn · 12 yield · 13 fill · 14 sleep · 15 brk · 60 exit

## 构建 / 测试

需要：clang 或 gcc、nasm、rustc(stable 即可)、qemu-system-x86_64、
mkbootimg(bootboot)。

```
make                        # 内核混合构建 C+asm+Rust
bash tools/ci.sh            # 全套: 77 项宿主单测 + 构建 + QEMU 启动 + 屏幕检查
rustc --edition 2021 --test tools/test_root.rs -o /tmp/t && /tmp/t
```

模块契约 / 任务表 / 通道表 / 内存布局：`docs/design.md`。

## 已知边界 / 下一步

- argv 参数：内核侧管道已验证，参数版镜像已在 tmpfs（`argv` 文件），
  用户态读参的端到端验证待完成
- 能力槽无回收：`run` 反复执行会耗尽 CSpace 槽位，需加回收机制
- 装载窗口已随任务槽回收（WINTAB 闭环），内存侧长期运行无泄漏
- 每进程独立地址空间（CR3 切换）封存于 rust_vm_bootstrap，待调试
- 翻译器后端目前是内核 tmpfs；接你的 fat12 磁盘是自然的下一步
- 语言策略：新增代码一律 Rust（no_std）

## 合流建议

test 对 main 的侵入只有 kernel.c 的 4 个钩子 + Makefile 两行 + docs/。
要合流：`git merge test` 冲突面极小；不合流也可独立演进（CI 全绿）。
