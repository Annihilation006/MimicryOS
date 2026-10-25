/* idt.h — 长模式 64 位中断描述符表（原 32 位版在长模式必崩，此为修正版） */
#ifndef IDT_H
#define IDT_H

#define IDT_ENTRIES 256
#define IRQ_BASE 32

/* x86_64 门描述符：16 字节，目标基址 64 位 */
typedef struct {
    unsigned short base_low;
    unsigned short selector;
    unsigned char ist;
    unsigned char flags;
    unsigned short base_mid;
    unsigned int base_high;
    unsigned int reserved;
} __attribute__((packed)) idt_entry_t;

typedef struct {
    unsigned short limit;
    unsigned long long base;
} __attribute__((packed)) idt_ptr_t;

/* 桩压栈布局：15 个通用寄存器 + int_no/err_code + iretq 的 5 件套 */
typedef struct {
    unsigned long long r15, r14, r13, r12, r11, r10, r9, r8;
    unsigned long long rbp, rdi, rsi, rdx, rcx, rbx, rax;
    unsigned long long int_no, err_code;
    unsigned long long rip, cs, rflags, rsp, ss;
} __attribute__((packed)) interrupt_frame_t;

/* 处理器返回新 rsp (0=不切换), 中断桩据此换栈 — 抢占式调度的机关 */
typedef unsigned long long (*interrupt_handler_t)(interrupt_frame_t*);

void idt_init(void);
void idt_set_gate(int num, unsigned long long base, unsigned short selector, unsigned char flags);
void register_interrupt_handler(int num, interrupt_handler_t handler);
void enable_interrupts(void);
void disable_interrupts(void);

extern interrupt_handler_t interrupt_handlers[];

#endif
