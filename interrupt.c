/* interrupt.c - 中断分发器（他的，frame 字段 32→64 位改名） */
#include "idt.h"
#include "console.h"
#include "pic.h"

extern interrupt_handler_t interrupt_handlers[];
static const char* exception_names[] = {
    "Division by zero", "Debug", "NMI", "Breakpoint",
    "Overflow", "Bound range", "Invalid opcode", "Device not available",
    "Double fault", "Coprocessor", "Invalid TSS", "Segment not present",
    "Stack fault", "GPF", "Page fault", "Reserved",
    "x87 FPU", "Alignment check", "Machine check", "SIMD FP",
    "Virtualization", "Control protection", "Reserved", "Reserved",
    "Reserved", "Reserved", "Reserved", "Reserved",
    "Reserved", "Reserved", "Security", "Reserved"
};

void interrupt_dispatcher(interrupt_frame_t* frame) {
    if (frame->int_no < 32) {
        print("\n!!! CPU Exception !!!\n  ");
        print(exception_names[frame->int_no]);
        print("\n  RIP: ");
        print_hex((unsigned int)(frame->rip >> 32));  /* 高 32 位先顶着, 反正内核住在高地址 */
        print_hex((unsigned int)frame->rip);
        print("\n  System halted!\n");
        while(1) { __asm__ volatile("hlt"); }
    }
}

void irq_dispatcher(interrupt_frame_t* frame) {
    int irq = (int)frame->int_no - IRQ_BASE;
    pic_send_eoi((unsigned char)irq);
    if (interrupt_handlers[frame->int_no]) {
        interrupt_handlers[frame->int_no](frame);
    }
}
