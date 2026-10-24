# MimicryOS

一个从零开始构建的 64 位自制操作系统。

- **架构**：x86_64
- **语言**：C + 少量汇编
- **引导**：BOOTBOOT
- **许可**：MIT
- **开发方式**：Human + AI 结对编程（架构设计由人类主导，AI 辅助编写代码与排错）

## 🔍 项目自述与开源声明

本仓库是一个纯粹的个人开源爱好项目，旨在从零学习并构建操作系统底层技术。为保证项目的透明度与合规性，特此声明：

### 1. 自研核心（完全从零实现）
- **系统架构**：模块化设计、Registry 模块注册加载机制。
- **内存管理**：基于双向链表、块分割与合并的堆管理（`kmalloc` / `kfree`），堆栈已映射至 `.bss` 段虚拟地址，解决长模式页表崩溃问题。
- **文件系统**：FAT12 风格的格式化、文件创建、读写及持久化逻辑。
- **硬件驱动**：ATA PIO 硬盘读写（含超时保护），VGA Framebuffer 图形输出，PS/2 键盘轮询（支持 Shift/CapsLock/方向键）。
- **Shell 框架**：完全自研的命令行行编辑器，支持光标移动、中间插入、退格删除及命令解析。

### 2. 引用的开源组件 (Dependencies)
本系统引用了以下优秀的开源组件，严格遵守其原始许可协议：
- **[BOOTBOOT](https://gitlab.com/bztsrc/bootboot)** (MIT)：作为 Bootloader 负责加载内核并进入 64 位长模式。
- **[stb_image](https://github.com/nothings/stb)** (MIT/Public Domain)：用于 PNG 图像解码支持。

### 3. 架构演进历史 (Refactoring History)
- **早期探索**：项目早期为了快速跑通 Shell 交互，曾移植并引用了 PlosClan 社区的 `pl_readline` 库作为命令行输入组件。
- **纯血重构**：为了保证内核的绝对精简与底层可控性，我们在近期的重构中，**已经将该库完全物理移除**，并由核心开发者纯手写实现了一套适配裸机环境的高效 Shell 行编辑器。当前源码树中已不包含任何第三方 OS 输入库的代码。

## 开发团队 (Contributors)
- **Annihilation006**：内核架构、内存管理、文件系统、Shell 核心。
- **ming_yv1**：底层驱动修复、工程结构重构、构建工具链整合。
- **Hugo_YuGuo**：负责后续桌面环境、窗口系统与用户态应用。

## 当前特性
- **系统**：64 位长模式、VGA Framebuffer 图形显示、PSF 字体渲染。
- **输入**：PS/2 键盘读取、方向键扫描码支持。
- **Shell**：命令解析、光标移动、20+ 内置命令。
- **文件系统**：FAT12 格式化、文件操作、磁盘持久化。
- **模块化**：支持模块注册/查找/列表，已有文件系统、鼠标、PNG、用户等模块。

## 项目结构

MimicryOS/
├── kernel.c              内核主程序（包含内存、ATA、VGA、键盘、Shell、模块核心）
├── *.mod.c               各个独立模块（文件系统、鼠标、PNG等）
├── bootboot.h            BOOTBOOT 协议头文件
├── font.psf              字体文件（通过 tools/genfont.py 生成）
├── tools/                构建辅助脚本
├── legacy/               历史遗留测试代码（已废弃，不参与当前构建）
├── linker.ld             链接脚本
└── Makefile              构建脚本

🛠️ 构建指南

依赖环境：Linux (推荐 Ubuntu)、clang、nasm、ld、make、qemu-system-x86_64

1. 编译内核

# 注意：需确保当前目录下有 font.psf，Makefile 会自动处理字体链接
make CC=clang

2. 生成启动镜像 (需配置 BOOTBOOT)

cp kernel.elf ~/bootboot-master/boot/kernel.elf
cd ~/bootboot-master
./mkbootimg/mkbootimg bootboot.json bootboot.img

3. 虚拟机运行

qemu-system-x86_64 -m 64M -drive file=bootboot.img,format=raw

4. 实体机运行 (⚠️ 警告：写入前请再三确认设备号)

sudo dd if=bootboot.img of=/dev/sdX bs=4M status=progress

🗺️ 路线图 (Roadmap)

P0 — 修复致命 Bug

☑ 修复长模式下堆栈物理地址引发的 Page Fault（已通过重构至 .bss 解决）
☐ 修复 idt_init() 加载后的崩溃
☐ 修复 int 0x80 系统调用的 Triple Fault

P1 — 核心筑基

☐ 实现多任务调度（上下文切换）
☐ 升级 FAT12 到 FAT16，支持子目录与长文件名 (VFAT)
☐ 补全系统目录结构（/system, /users, /apps 等）
☐ 实现 PIT 定时器与 get_tick_ms()

P2 — 图形与接口

☐ 鼠标指针渲染与事件监听
☐ 实现基础窗口系统与桌面图标（与 Rust 开发者协作）
☐ 开放更底层的绘图接口（矩形、线条、双缓冲）

P3 — 用户态与生态

☐ 分页机制与用户态 (Ring 3)
☐ .mex 可执行文件加载器
☐ 实现基础文件管理器（查看/新建 .txt 等文本文件）
☐ 设计 .mim 安装包格式
☐ 移植浏览器与尝试联网功能

🥚 副线任务

☐ 添加系统隐藏彩蛋

致谢

· BOOTBOOT — 引导加载程序
· stb_image — 图像解码库
· OSDev Wiki — 操作系统开发知识库

许可证

本项目采用 MIT License 开源。

运行

【又一个操作系统来了！-哔哩哔哩】 https://b23.tv/UrL2fwN
