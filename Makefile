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

OBJS = kernel.o filesystem.mod.o mouse.mod.o desktop.mod.o png.mod.o font.o cpu.o idt.o interrupt.o pic.o console.o isr.o usermode.o vmo.o

all: kernel.elf

# ── test 分支新增：Rust(no_std) 对象 ─────────────────────────────
# 检测到 rustc 时编入 rust_kernel.o（no_std，与 C/asm 同链一个 ELF）；
# 未安装则打印提示并跳过，保证在只有 C 工具链的机器上仍可完整构建。
# 注意：本块必须位于 all: 之后，避免 rust_kernel.o 规则抢占默认目标。
RUSTC ?= rustc
RUST_OK := $(shell command -v rustc >/dev/null 2>&1 && echo yes)
ifeq ($(RUST_OK),yes)
OBJS += rust_kernel.o
rust_kernel.o: rust_kernel.rs frame.rs kmem.rs vma.rs pagetable.rs vmboot.rs aspace.rs capability.rs ipc.rs task.rs syscall.rs userland.rs userland2.rs userland3.rs userland4.rs tmpfs.rs elf.rs translator.rs elfprog.rs
	$(RUSTC) --crate-type lib --target x86_64-unknown-none --edition 2021 \
	    -C opt-level=2 --emit=obj -o $@ $<
else
$(info [test] 未检测到 rustc，跳过 Rust 对象（安装 rustup 后自动启用）)
endif

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

# test 分支新增：x86_64 汇编 CPU 原语（端口IO/控制寄存器/CPUID/RDTSC/MSR）
cpu.o: cpu.asm
	$(ASM) $(ASMFLAGS) cpu.asm -o cpu.o

# test 分支新增：中断桩（48 个 isr/irq + idt_load）
isr.o: isr.asm
	$(ASM) $(ASMFLAGS) isr.asm -o isr.o

# test 分支新增：Ring3 用户态全家桶（纯汇编）+ VMO 共享内存（纯汇编）
usermode.o: usermode.asm
	$(ASM) $(ASMFLAGS) usermode.asm -o usermode.o

vmo.o: vmo.asm
	$(ASM) $(ASMFLAGS) vmo.asm -o vmo.o

kernel.elf: $(OBJS) linker.ld
	$(LD) -m elf_x86_64 -T linker.ld --no-pie -o kernel.elf $(OBJS)

clean:
	rm -f *.o *.elf font.o

.PHONY: all clean
