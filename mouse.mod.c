#include "module.h"

void fb_print(const char* str);
void fb_putpixel(int x, int y, unsigned int color);
unsigned char inb(unsigned short port);
void outb(unsigned short port, unsigned char val);

static int mouse_x = 400;
static int mouse_y = 300;
static int mouse_button = 0;

void mouse_init(void) {
    outb(0x64, 0xA8);
    outb(0x64, 0x20);
    unsigned char status = inb(0x60) | 2;
    outb(0x64, 0x60);
    outb(0x60, status);
    outb(0x64, 0xD4);
    outb(0x60, 0xF4);
}

void mouse_update(void) {
    if (!(inb(0x64) & 0x01)) return;
    unsigned char byte1 = inb(0x60);
    if (!(inb(0x64) & 0x01)) return;
    unsigned char byte2 = inb(0x60);
    if (!(inb(0x64) & 0x01)) return;
    unsigned char byte3 = inb(0x60);
    mouse_button = byte1 & 0x07;
    mouse_x += (int)((char)byte2);
    mouse_y -= (int)((char)byte3);
    if (mouse_x < 0) mouse_x = 0;
    if (mouse_y < 0) mouse_y = 0;
    if (mouse_x > 800) mouse_x = 800;
    if (mouse_y > 600) mouse_y = 600;
}

void mouse_draw(void) {
    for (int i = 0; i < 10; i++) {
        fb_putpixel(mouse_x + i, mouse_y + i, 0x000000);
        fb_putpixel(mouse_x + i, mouse_y, 0x000000);
        fb_putpixel(mouse_x, mouse_y + i, 0x000000);
    }
}

static int mouse_module_init(void) {
    mouse_init();

    return 0;
}

MODULE_EXPORT module_t mouse_module = {
    .magic = MODULE_MAGIC,
    .name = "mouse",
    .desc = "PS/2 Mouse driver",
    .init = mouse_module_init,
    .next = NULL
};
