; ============================================================
; usermode.asm — Ring 3 基础设施（跳板层, 逻辑全部在 Rust）
; GDT/TSS 自建, 页表 U/S 修补, int 0x80 门, 用户栈
; 系统调用分发/字库/用户载荷 → syscall.rs / userland.rs (Rust)
; ABI 按设计文档 07: rax=号 rdi=能力令牌 rsi/rdx/r8=参数
; ============================================================

[bits 64]

extern idt_set_gate
extern rust_syscall_entry

section .data
align 16
gdt:
    dq 0                          ; null
    dq 0x00AF9A000000FFFF         ; 0x08 kcode L=1
    dq 0x00CF92000000FFFF         ; 0x10 kdata
    dq 0x0020FA000000FFFF         ; 0x18 ucode ring3 L=1
    dq 0x00CFF2000000FFFF         ; 0x20 udata ring3
    dq 0                          ; 0x28 tss 低 8B (运行时填)
    dq 0                          ;       tss 高 8B
gdtr:
    dw 63
    dq gdt

section .bss
align 16
tss:        resb 128
irq_stack0: resq 512            ; ring3 进来的内核栈 (TSS RSP0)
user_stack: resq 512            ; 用户栈

section .text

; ---------------- 初始化: lgdt/ltr + 页表放行 + 开 0x80 门 ----------------
global usermode_init
usermode_init:
    ; TSS 描述符 (16B) 手搓
    mov rax, tss
    mov rbx, 103
    mov rcx, rax
    and rcx, 0xFFFF
    shl rcx, 16
    or rbx, rcx                 ; limit15_0 | base15_0<<16
    mov rcx, rax
    shr rcx, 16
    and rcx, 0xFF
    shl rcx, 32
    or rbx, rcx                 ; base23_16<<32
    mov rcx, 0x89
    shl rcx, 40
    or rbx, rcx                 ; type=0x89 available 64bit TSS, P=1
    mov rcx, 103
    shr rcx, 16
    and rcx, 0xF
    shl rcx, 48
    or rbx, rcx                 ; limit19_16<<48
    mov rcx, rax
    shr rcx, 24
    and rcx, 0xFF
    shl rcx, 56
    or rbx, rcx                 ; base31_24<<56
    mov [gdt + 5*8], rbx
    mov rbx, rax
    shr rbx, 32
    and rbx, 0xFFFFFFFF
    mov [gdt + 6*8], rbx        ; base63_32

    lgdt [gdtr]
    push qword 0x08
    lea rax, [rel .flush]
    push rax
    retfq
.flush:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov ax, 0x0
    mov fs, ax
    mov gs, ax

    mov ax, 0x28
    ltr ax

    ; RSP0 = irq_stack0 顶 (TSS 偏移 4)
    lea rax, [irq_stack0 + 4096]
    mov [tss + 4], rax

    ; (页表放行改由 Rust 侧 patch_user_region 走直映射完成)

    ; IDT 128 号门: DPL=3 让 ring3 能 int
    mov edi, 128
    mov rsi, strict qword syscall_int_entry
    mov edx, 0x08
    mov ecx, 0xEE
    call idt_set_gate
    ret

; rcx = va, 沿 PML4→PDPT→PD 全路 U/S 置位 + NX 清位
; 末级 4K 页时整 PD 的 512 个 PTE 全放行（.data 掉哪页都不怕）
global syscall_int_entry
syscall_int_entry:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    mov rdi, rsp                    ; SyscallFrame*
    call rust_syscall_entry
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    iretq

global user_stack_top
user_stack_top equ user_stack + 4096
