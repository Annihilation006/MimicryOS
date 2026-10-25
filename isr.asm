; isr.asm — 48 个中断桩 + 公共入口 + idt_load（补完他的骨架）
; 桩统一: [有错码?] push int_no → 公共入口推 15 个寄存器 → C 分发器
[BITS 64]

extern interrupt_dispatcher
extern irq_dispatcher

%macro PUSHA64 0
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
%endmacro

%macro POPA64 0
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
%endmacro

; 无错码的异常/IRQ: 补个假错码
%macro STUB 1
global isr%1
isr%1:
    push qword 0
    push qword %1
    jmp common_stub
%endmacro

; 8,10-14,17,21 自带错码
%macro STUB_ERR 1
global isr%1
isr%1:
    push qword %1
    jmp common_stub
%endmacro

%macro IRQSTUB 1
global irq%1
irq%1:
    push qword 0
    push qword %1
    jmp common_irq
%endmacro

common_stub:
    PUSHA64
    mov rdi, rsp
    cld
    call interrupt_dispatcher
    POPA64
    add rsp, 16
    iretq

common_irq:
    PUSHA64
    mov rdi, rsp
    cld
    call irq_dispatcher
    POPA64
    add rsp, 16
    iretq

; 异常: 8/10/11/12/13/14/17/21 有错码, 其余补 0
STUB 0
STUB 1
STUB 2
STUB 3
STUB 4
STUB 5
STUB 6
STUB 7
STUB_ERR 8
STUB 9
STUB_ERR 10
STUB_ERR 11
STUB_ERR 12
STUB_ERR 13
STUB_ERR 14
STUB 15
STUB 16
STUB_ERR 17
STUB 18
STUB 19
STUB 20
STUB_ERR 21
STUB 22
STUB 23
STUB 24
STUB 25
STUB 26
STUB 27
STUB 28
STUB 29
STUB 30
STUB 31

IRQSTUB 32
IRQSTUB 33
IRQSTUB 34
IRQSTUB 35
IRQSTUB 36
IRQSTUB 37
IRQSTUB 38
IRQSTUB 39
IRQSTUB 40
IRQSTUB 41
IRQSTUB 42
IRQSTUB 43
IRQSTUB 44
IRQSTUB 45
IRQSTUB 46
IRQSTUB 47

; C 侧: idt_load((u64)&idt_ptr), SysV 第一参在 rdi
global idt_load
idt_load:
    lidt [rdi]
    ret

; ---- 他原有的入口, 保留 ----
; (irq_handler_entry 随 945 行版 kernel.c 移植时再启用, 现在 irq_handler 还没有)

global syscall_entry
extern syscall_handler

syscall_entry:
    mov rax, 42
    iretq
