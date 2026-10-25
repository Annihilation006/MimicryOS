CC = gcc
CFLAGS = -m64 -mcmodel=large -ffreestanding -fno-pie -fno-stack-protector -nostdlib -Wall -Wextra -O0 -I.
ASM = nasm
ASMFLAGS = -f elf64

OBJS = kernel.o isr.o pl_stub.o plreadln.o plreadln_color.o plreadln_history.o plreadln_intellisense.o plreadln_util.o plreadln_wordmk.o filesystem.mod.o mouse.mod.o desktop.mod.o png.mod.o

all: kernel.elf

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

%.o: %.asm
	nasm -f elf64 $< -o $@

kernel.elf: $(OBJS) linker.ld
	ld -m elf_x86_64 -T linker.ld --no-pie -o kernel.elf $(OBJS) font.o

clean:
	rm -f *.o *.elf

.PHONY: all clean
