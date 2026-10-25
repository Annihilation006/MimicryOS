# MimicryOS

一个从零开始构建的 64 位自制操作系统。

- **架构**：x86_64 长模式
- **语言**：C + 少量汇编
- **引导**：BOOTBOOT
- **许可**：MIT License
- **开发方式**：Human + AI 结对编程
- **开发者**：Annihilation006、ming_yv1、Hugo_YuGuo

## 🔍 项目自述

本仓库是一个纯粹的个人开源爱好项目，旨在从零学习并构建操作系统底层技术。
- **自研核心**：模块化架构、物理内存堆管理、FAT12 文件系统、PS/2 键盘、VGA 图形与 Shell 环境。
- **纯血重构**：早期的 `pl_readline` 库已被完全物理移除，Shell 行编辑器已由核心开发者手写重写，内核精简可控。

## 📦 引用的开源组件 (Dependencies)

本系统引用了以下优秀的开源组件，并严格遵守其原始许可协议：
- **BOOTBOOT** (MIT)：Bootloader，负责加载内核并进入 64 位长模式。
- **stb_image** (MIT / Public Domain)：用于 PNG 图像解码支持。
- **Spleen 8x16** (BSD-2-Clause)：系统内置 `font.psf` 字体来源。

*(关于字体的详细来源与 BSD-2-Clause 完整许可，请参阅 [FONT-NOTICE.txt](FONT-NOTICE.txt))*

## 🛠️ 构建指南

**依赖环境**：
- Linux 或 Windows (MSYS2 UCRT64)
- `clang` 或 `gcc`、`ld`、`nasm`、`make`、`qemu-system-x86_64`
- 字体转换：`python`（需安装 `tools/genfont.py` 的依赖）

**编译步骤**：

# 1. 生成字体（如果当前目录下没有 font.psf）
python tools/genfont.py

# 2. 编译内核
make CC=clang LD=ld.lld   # Windows 下建议指定 lld

# 3. 打包启动镜像 (需配置 BOOTBOOT)
./bootboot-master/mkbootimg/mkbootimg iso_build/myos.json iso_build/myos.img

# 4. 虚拟机运行
qemu-system-x86_64 -hda iso_build/myos.img -m 1024M -boot d

(注意：实体机运行需用 dd 将 img 写入 U 盘，请务必核对设备号，后果自负)

🗺️ 路线图 (Roadmap)

☑ 修复长模式下堆栈物理地址引发的 Page Fault（已重构至 .bss）
☐ 修复 idt_init() 加载后的崩溃
☐ 修复 int 0x80 系统调用的 Triple Fault
☐ 实现多任务调度（上下文切换）与 PIT 定时器
☐ 升级 FAT12 到 FAT16/VFAT，支持子目录与长文件名
☐ 实现基础窗口系统与桌面图标
☐ 分页机制与用户态 (Ring 3)

📄 许可证 (License)

本项目采用 MIT License 开源。
（包含的第三方组件和字体归其原作者所有，受各自许可证保护。）
