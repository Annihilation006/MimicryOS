/* idt.c - 中断描述符表（他的骨架，门描述符修正为长模式 16 字节版） */
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

extern void idt_load(unsigned long long);

void idt_set_gate(int num, unsigned long long base, unsigned short sel, unsigned char flags) {
    idt_entries[num].base_low = base & 0xFFFF;
    idt_entries[num].base_mid = (base >> 16) & 0xFFFF;
    idt_entries[num].base_high = (base >> 32) & 0xFFFFFFFF;
    idt_entries[num].selector = sel;
    idt_entries[num].ist = 0;
    idt_entries[num].flags = flags;
    idt_entries[num].reserved = 0;
}

void idt_init(void) {
    idt_ptr.limit = sizeof(idt_entry_t) * IDT_ENTRIES - 1;
    idt_ptr.base = (unsigned long long)&idt_entries;

    for (int i = 0; i < IDT_ENTRIES; i++) {
        idt_set_gate(i, 0, 0, 0);
        interrupt_handlers[i] = 0;
    }

    // CPU异常
    idt_set_gate(0,  (unsigned long long)isr0,  0x08, 0x8E);
    idt_set_gate(1,  (unsigned long long)isr1,  0x08, 0x8E);
    idt_set_gate(2,  (unsigned long long)isr2,  0x08, 0x8E);
    idt_set_gate(3,  (unsigned long long)isr3,  0x08, 0x8E);
    idt_set_gate(4,  (unsigned long long)isr4,  0x08, 0x8E);
    idt_set_gate(5,  (unsigned long long)isr5,  0x08, 0x8E);
    idt_set_gate(6,  (unsigned long long)isr6,  0x08, 0x8E);
    idt_set_gate(7,  (unsigned long long)isr7,  0x08, 0x8E);
    idt_set_gate(8,  (unsigned long long)isr8,  0x08, 0x8E);
    idt_set_gate(9,  (unsigned long long)isr9,  0x08, 0x8E);
    idt_set_gate(10, (unsigned long long)isr10, 0x08, 0x8E);
    idt_set_gate(11, (unsigned long long)isr11, 0x08, 0x8E);
    idt_set_gate(12, (unsigned long long)isr12, 0x08, 0x8E);
    idt_set_gate(13, (unsigned long long)isr13, 0x08, 0x8E);
    idt_set_gate(14, (unsigned long long)isr14, 0x08, 0x8E);
    idt_set_gate(15, (unsigned long long)isr15, 0x08, 0x8E);
    idt_set_gate(16, (unsigned long long)isr16, 0x08, 0x8E);
    idt_set_gate(17, (unsigned long long)isr17, 0x08, 0x8E);
    idt_set_gate(18, (unsigned long long)isr18, 0x08, 0x8E);
    idt_set_gate(19, (unsigned long long)isr19, 0x08, 0x8E);
    idt_set_gate(20, (unsigned long long)isr20, 0x08, 0x8E);
    idt_set_gate(21, (unsigned long long)isr21, 0x08, 0x8E);
    idt_set_gate(22, (unsigned long long)isr22, 0x08, 0x8E);
    idt_set_gate(23, (unsigned long long)isr23, 0x08, 0x8E);
    idt_set_gate(24, (unsigned long long)isr24, 0x08, 0x8E);
    idt_set_gate(25, (unsigned long long)isr25, 0x08, 0x8E);
    idt_set_gate(26, (unsigned long long)isr26, 0x08, 0x8E);
    idt_set_gate(27, (unsigned long long)isr27, 0x08, 0x8E);
    idt_set_gate(28, (unsigned long long)isr28, 0x08, 0x8E);
    idt_set_gate(29, (unsigned long long)isr29, 0x08, 0x8E);
    idt_set_gate(30, (unsigned long long)isr30, 0x08, 0x8E);
    idt_set_gate(31, (unsigned long long)isr31, 0x08, 0x8E);

    // 硬件中断
    idt_set_gate(32, (unsigned long long)irq32, 0x08, 0x8E);
    idt_set_gate(33, (unsigned long long)irq33, 0x08, 0x8E);
    idt_set_gate(34, (unsigned long long)irq34, 0x08, 0x8E);
    idt_set_gate(35, (unsigned long long)irq35, 0x08, 0x8E);
    idt_set_gate(36, (unsigned long long)irq36, 0x08, 0x8E);
    idt_set_gate(37, (unsigned long long)irq37, 0x08, 0x8E);
    idt_set_gate(38, (unsigned long long)irq38, 0x08, 0x8E);
    idt_set_gate(39, (unsigned long long)irq39, 0x08, 0x8E);
    idt_set_gate(40, (unsigned long long)irq40, 0x08, 0x8E);
    idt_set_gate(41, (unsigned long long)irq41, 0x08, 0x8E);
    idt_set_gate(42, (unsigned long long)irq42, 0x08, 0x8E);
    idt_set_gate(43, (unsigned long long)irq43, 0x08, 0x8E);
    idt_set_gate(44, (unsigned long long)irq44, 0x08, 0x8E);
    idt_set_gate(45, (unsigned long long)irq45, 0x08, 0x8E);
    idt_set_gate(46, (unsigned long long)irq46, 0x08, 0x8E);
    idt_set_gate(47, (unsigned long long)irq47, 0x08, 0x8E);

    idt_load((unsigned long long)&idt_ptr);
    print("IDT initialized.\n");
}

void register_interrupt_handler(int num, interrupt_handler_t handler) {
    interrupt_handlers[num] = handler;
}

void enable_interrupts(void) {
    __asm__ volatile("sti");
}

void disable_interrupts(void) {
    __asm__ volatile("cli");
}
