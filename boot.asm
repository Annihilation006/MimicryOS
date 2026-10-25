[BITS 32]

MBALIGN     equ 1<<0
MEMINFO     equ 1<<1
FLAGS       equ MBALIGN | MEMINFO
MAGIC       equ 0x1BADB002
CHECKSUM    equ -(MAGIC + FLAGS)

section .multiboot
align 4
    dd MAGIC
    dd FLAGS
    dd CHECKSUM
    dd 0x00100000   ; header_addr
    dd 0x00100000   ; load_addr
    dd 0            ; load_end_addr
    dd 0            ; bss_end_addr
    dd 0            ; entry_addr

section .text
global _start
_start:
    mov eax, 0xB8000
    mov word [eax], 0x0F4F

.hang:
    hlt
    jmp .hang