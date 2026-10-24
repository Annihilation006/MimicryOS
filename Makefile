# ── 工具链选择 ─────────────────────────────────────────────────────────
# 本工程产出真正的 ELF64 内核（BOOTBOOT 协议）。
#
#   Windows (MSYS2)：自带 gcc 只能出 PE 目标，做不了 ELF64，
#                     所以这里用 clang --target=x86_64-none-elf + ld.lld。
#   Linux：系统 gcc 原生就是 ELF64，无需 --target，直接能编。
#
# 规则：默认优先用 clang/lld（若装了），否则自动回退到 gcc/ld。
#       也可手动指定：  make CC=gcc LD=ld   （Linux 上没有 clang 时）
#                       make CC=clang LD=ld.lld
# 先装依赖：
#   Windows:  pacman -S --needed mingw-w64-ucrt-x86_64-clang mingw-w64-ucrt-x86_64-lld
#   Linux:    用发行版自带 gcc/binutils 即可（cc、ld、objcopy、nasm 可选）
# ─────────────────────────────────────────────────────────────────────────

ifeq ($(origin CC),default)
  CC := $(shell (command -v clang >/dev/null 2>&1 && echo clang) || echo gcc)
endif
ifeq ($(origin LD),default)
  LD := $(shell (command -v ld.lld >/dev/null 2>&1 && echo ld.lld) || echo ld)
endif

# clang 需要 --target 指定裸机 ELF64；gcc 原生就是 ELF64，不用加
ifeq ($(shell echo '$(CC)' | grep -q clang && echo clang),clang)
  TARGETFLAG := --target=x86_64-none-elf
else
  TARGETFLAG :=
endif

CFLAGS = $(TARGETFLAG) -m64 -mcmodel=large -ffreestanding -fno-pie -fno-stack-protector -nostdlib -isystem . -Wall -Wextra -O0
ASM = nasm
ASMFLAGS = -f elf64
OBJCOPY = objcopy

OBJS = kernel.o filesystem.mod.o mouse.mod.o desktop.mod.o png.mod.o font.o

all: kernel.elf

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

# stb_image.h 编译时因宏裁剪会产生未使用的 static 函数告警，
# 仅对 png 目标关闭 unused-function，不影响其他源文件的告警级别。
png.mod.o: png.mod.c stb_image.h
	$(CC) $(CFLAGS) -Wno-unused-function -c $< -o $@

# font.psf 是 8x16 的 PSF2 字体，由脚本 tools/genfont.py 生成。
# objcopy 把二进制字体包成 ELF 目标文件，导出 _binary_font_psf_start 等符号。
font.o: font.psf
	$(OBJCOPY) -I binary -O elf64-x86-64 -B i386:x86-64 font.psf font.o

kernel.elf: $(OBJS) linker.ld
	$(LD) -m elf_x86_64 -T linker.ld --no-pie -o kernel.elf $(OBJS)

clean:
	rm -f *.o *.elf font.o

.PHONY: all clean
