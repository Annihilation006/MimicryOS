#ifndef IDT_H
#define IDT_H

#define IDT_ENTRIES 256
#define IRQ_BASE 32

typedef struct {
    unsigned short base_low;
    unsigned short selector;
    unsigned char zero;
    unsigned char flags;
    unsigned short base_high;
} __attribute__((packed)) idt_entry_t;

typedef struct {
    unsigned short limit;
    unsigned int base;
} __attribute__((packed)) idt_ptr_t;

typedef struct {
    unsigned int ds;
    unsigned int edi, esi, ebp, esp, ebx, edx, ecx, eax;
    unsigned int int_no, err_code;
    unsigned int eip, cs, eflags, useresp, ss;
} interrupt_frame_t;

typedef void (*interrupt_handler_t)(interrupt_frame_t*);

void idt_init(void);
void idt_set_gate(int num, unsigned int base, unsigned short selector, unsigned char flags);
void register_interrupt_handler(int num, interrupt_handler_t handler);
void enable_interrupts(void);
void disable_interrupts(void);

extern interrupt_handler_t interrupt_handlers[];

#endif