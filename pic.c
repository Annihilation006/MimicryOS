/* pic.c */
#include "pic.h"
#include "console.h"

void pic_remap(void) {
    outb(0x20, 0x11);
    outb(0xA0, 0x11);
    outb(0x21, 0x20);
    outb(0xA1, 0x28);
    outb(0x21, 0x04);
    outb(0xA1, 0x02);
    outb(0x21, 0x01);
    outb(0xA1, 0x01);
    outb(0x21, 0xFF);
    outb(0xA1, 0xFF);
    print("PIC remapped.\n");
}

void pic_set_mask(unsigned char irq) {
    unsigned short port = (irq < 8) ? 0x21 : 0xA1;
    if (irq >= 8) irq -= 8;
    outb(port, inb(port) | (1 << irq));
}

void pic_clear_mask(unsigned char irq) {
    unsigned short port = (irq < 8) ? 0x21 : 0xA1;
    if (irq >= 8) irq -= 8;
    outb(port, inb(port) & ~(1 << irq));
}

void pic_send_eoi(unsigned char irq) {
    if (irq >= 8) outb(0xA0, 0x20);
    outb(0x20, 0x20);
}