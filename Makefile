CC = gcc
CFLAGS = -m64 -mcmodel=large -ffreestanding -fno-pie -fno-stack-protector -nostdlib -Wall -Wextra -O0
ASM = nasm
ASMFLAGS = -f elf64

OBJS = kernel.o filesystem.mod.o mouse.mod.o desktop.mod.o png.mod.o

all: kernel.elf

%.o: %.c
	$(CC) $(CFLAGS) -c $< -o $@

kernel.elf: $(OBJS) linker.ld
	ld -m elf_x86_64 -T linker.ld --no-pie -o kernel.elf $(OBJS) font.o

clean:
	rm -f *.o *.elf

.PHONY: all clean
