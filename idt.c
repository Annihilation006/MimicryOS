/* idt.c - 中断描述符表 */
#include "idt.h"
#include "console.h"

extern void isr0();  extern void isr1();  extern void isr2();  extern void isr3();
extern void isr4();  extern void isr5();  extern void isr6();  extern void isr7();
extern void isr8();  extern void isr9();  extern void isr10(); extern void isr11();
extern void isr12(); extern void isr13(); extern void isr14(); extern void isr15();
extern void isr16(); extern void isr17(); extern void isr18(); extern void isr19();
extern void isr20(); extern void isr21(); extern void isr22(); extern void isr23();
extern void isr24(); extern void isr25(); extern void isr26(); extern void isr27();
extern void isr28(); extern void isr29(); extern void isr30(); extern void isr31();
extern void irq32(); extern void irq33(); extern void irq34(); extern void irq35();
extern void irq36(); extern void irq37(); extern void irq38(); extern void irq39();
extern void irq40(); extern void irq41(); extern void irq42(); extern void irq43();
extern void irq44(); extern void irq45(); extern void irq46(); extern void irq47();

static idt_entry_t idt_entries[IDT_ENTRIES];
static idt_ptr_t idt_ptr;
interrupt_handler_t interrupt_handlers[IDT_ENTRIES];

extern void idt_load(unsigned int);

void idt_set_gate(int num, unsigned int base, unsigned short sel, unsigned char flags) {
    idt_entries[num].base_low = base & 0xFFFF;
    idt_entries[num].base_high = (base >> 16) & 0xFFFF;
    idt_entries[num].selector = sel;
    idt_entries[num].zero = 0;
    idt_entries[num].flags = flags;
}

void idt_init(void) {
    idt_ptr.limit = sizeof(idt_entry_t) * IDT_ENTRIES - 1;
    idt_ptr.base = (unsigned int)&idt_entries;
    
    for (int i = 0; i < IDT_ENTRIES; i++) {
        idt_set_gate(i, 0, 0, 0);
        interrupt_handlers[i] = 0;
    }
    
    // CPU异常
    idt_set_gate(0,  (unsigned int)isr0,  0x08, 0x8E);
    idt_set_gate(1,  (unsigned int)isr1,  0x08, 0x8E);
    idt_set_gate(2,  (unsigned int)isr2,  0x08, 0x8E);
    idt_set_gate(3,  (unsigned int)isr3,  0x08, 0x8E);
    idt_set_gate(4,  (unsigned int)isr4,  0x08, 0x8E);
    idt_set_gate(5,  (unsigned int)isr5,  0x08, 0x8E);
    idt_set_gate(6,  (unsigned int)isr6,  0x08, 0x8E);
    idt_set_gate(7,  (unsigned int)isr7,  0x08, 0x8E);
    idt_set_gate(8,  (unsigned int)isr8,  0x08, 0x8E);
    idt_set_gate(9,  (unsigned int)isr9,  0x08, 0x8E);
    idt_set_gate(10, (unsigned int)isr10, 0x08, 0x8E);
    idt_set_gate(11, (unsigned int)isr11, 0x08, 0x8E);
    idt_set_gate(12, (unsigned int)isr12, 0x08, 0x8E);
    idt_set_gate(13, (unsigned int)isr13, 0x08, 0x8E);
    idt_set_gate(14, (unsigned int)isr14, 0x08, 0x8E);
    idt_set_gate(15, (unsigned int)isr15, 0x08, 0x8E);
    idt_set_gate(16, (unsigned int)isr16, 0x08, 0x8E);
    idt_set_gate(17, (unsigned int)isr17, 0x08, 0x8E);
    idt_set_gate(18, (unsigned int)isr18, 0x08, 0x8E);
    idt_set_gate(19, (unsigned int)isr19, 0x08, 0x8E);
    idt_set_gate(20, (unsigned int)isr20, 0x08, 0x8E);
    idt_set_gate(21, (unsigned int)isr21, 0x08, 0x8E);
    idt_set_gate(22, (unsigned int)isr22, 0x08, 0x8E);
    idt_set_gate(23, (unsigned int)isr23, 0x08, 0x8E);
    idt_set_gate(24, (unsigned int)isr24, 0x08, 0x8E);
    idt_set_gate(25, (unsigned int)isr25, 0x08, 0x8E);
    idt_set_gate(26, (unsigned int)isr26, 0x08, 0x8E);
    idt_set_gate(27, (unsigned int)isr27, 0x08, 0x8E);
    idt_set_gate(28, (unsigned int)isr28, 0x08, 0x8E);
    idt_set_gate(29, (unsigned int)isr29, 0x08, 0x8E);
    idt_set_gate(30, (unsigned int)isr30, 0x08, 0x8E);
    idt_set_gate(31, (unsigned int)isr31, 0x08, 0x8E);
    
    // 硬件中断
    idt_set_gate(32, (unsigned int)irq32, 0x08, 0x8E);
    idt_set_gate(33, (unsigned int)irq33, 0x08, 0x8E);
    idt_set_gate(34, (unsigned int)irq34, 0x08, 0x8E);
    idt_set_gate(35, (unsigned int)irq35, 0x08, 0x8E);
    idt_set_gate(36, (unsigned int)irq36, 0x08, 0x8E);
    idt_set_gate(37, (unsigned int)irq37, 0x08, 0x8E);
    idt_set_gate(38, (unsigned int)irq38, 0x08, 0x8E);
    idt_set_gate(39, (unsigned int)irq39, 0x08, 0x8E);
    idt_set_gate(40, (unsigned int)irq40, 0x08, 0x8E);
    idt_set_gate(41, (unsigned int)irq41, 0x08, 0x8E);
    idt_set_gate(42, (unsigned int)irq42, 0x08, 0x8E);
    idt_set_gate(43, (unsigned int)irq43, 0x08, 0x8E);
    idt_set_gate(44, (unsigned int)irq44, 0x08, 0x8E);
    idt_set_gate(45, (unsigned int)irq45, 0x08, 0x8E);
    idt_set_gate(46, (unsigned int)irq46, 0x08, 0x8E);
    idt_set_gate(47, (unsigned int)irq47, 0x08, 0x8E);
    
    idt_load((unsigned int)&idt_ptr);
    print("IDT initialized.\n");
}

void register_interrupt_handler(int num, interrupt_handler_t handler) {
    interrupt_handlers[num] = handler ;
}  

void enable_interrupts(void) {
    __asm__  volatile("sti");
}

void disable_interrupts(void) {
    __asm__ volatile("cli");
}