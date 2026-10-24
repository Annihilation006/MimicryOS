# MimicryOS 构建与镜像打包说明

## 1. 编译内核（Linux / Windows 通用）

依赖：
- **Linux**：发行版自带 gcc + binutils（`ld`、`objcopy`）；`nasm` 可选（当前未用汇编）
- **Windows (MSYS2 UCRT64)**：clang + lld
  `pacman -S --needed mingw-w64-ucrt-x86_64-clang mingw-w64-ucrt-x86_64-lld`

```
make clean
make
```

Makefile 会自动选择工具链：**装了 clang/lld 优先用**，否则回退到 gcc/ld。
也可手动指定（Linux 没装 clang 时最省事）：

```
make CC=gcc LD=ld
```

产物 `kernel.elf`：64 位 ELF，符合 BOOTBOOT 协议 Level 1 / 2（`mkbootimg check` 可验证）。

## 2. 打包可启动镜像（BOOTBOOT 的 mkbootimg）

`mkbootimg` 源码在 `bootboot-master/mkbootimg/`。

Linux 上先编译 mkbootimg：

```
cd bootboot-master/mkbootimg
make          # 产出 mkbootimg 可执行文件
```

打包（配置文件在 `iso_build/myos.json`）：

```
./bootboot-master/mkbootimg/mkbootimg iso_build/myos.json iso_build/myos.img
```

要点：
- `iso_build/boot/` 里放 `kernel.elf` 与 `sys/config`，其中 `sys/config` 内容逐字为 `kernel=kernel.elf`（**不能有空格**，否则 mkbootimg 匹配失败回退默认名）
- 产物 `iso_build/myos.img` 是混合镜像（MBR + GPT + ISO9660），64MB

使用：
- **QEMU 测试**：`qemu-system-x86_64 -hda iso_build/myos.img -m 1024M -boot d -k en-us`
- **实体机 / U 盘**：用 `dd` 或写盘工具写入
  `dd if=iso_build/myos.img of=/dev/sdX bs=4M`（⚠️ 目标盘会被整个清空）

## 3. 已实现 / 遗留

- 已有：32/64 位引导、VGA framebuffer + PSF 字体、键盘轮询 + Shift/CapsLock/方向键、Shell + 命令历史、kmalloc/kfree 堆、模块 registry、FAT12 持久化（格式化/建/写/读/删/列）、ATA 读写、开机动画、PNG 解码、int 0x80 框架（未接 IDT）
- 遗留：FAT12 数据区固定单扇区 LBA2890、根目录 16 项、8.3 文件名（无子目录/长文件名/多簇链）；中断驱动键盘/鼠标指针/USB/网卡；多任务与用户态（Ring3）为从零新功能，暂缺
