#include "module.h"

typedef unsigned long long uint64_t;
typedef unsigned int uint32_t;
typedef unsigned short uint16_t;
typedef unsigned char uint8_t;
typedef short int16_t;
#include "bootboot.h"

void fb_putpixel(int x, int y, unsigned int color);
void fb_print(const char* str);
void png_load(void* data, int size);
void png_render(int x, int y);
extern BOOTBOOT bootboot;

void draw_window(int x, int y, int w, int h) {
    for (int i = y; i < y + h; i++) {
        for (int j = x; j < x + w; j++) {
            fb_putpixel(j, i, 0xFFFFFF);
        }
    }
    for (int i = y; i < y + 20; i++) {
        for (int j = x; j < x + w; j++) {
            fb_putpixel(j, i, 0x000080);
        }
    }
    for (int i = x; i < x + w; i++) {
        fb_putpixel(i, y, 0x000000);
        fb_putpixel(i, y + h - 1, 0x000000);
    }
    for (int i = y; i < y + h; i++) {
        fb_putpixel(x, i, 0x000000);
        fb_putpixel(x + w - 1, i, 0x000000);
    }
}

void desktop_render_png(void) {
    unsigned char* ptr = (unsigned char*)bootboot.initrd_ptr;
    unsigned char* end = ptr + bootboot.initrd_size;

    while (ptr < end) {
        char name[101];
        int i;
        for (i = 0; i < 100 && ptr[i]; i++) name[i] = ptr[i];
        name[i] = '\0';
        if (name[0] == '\0') break;

        unsigned int size = 0;
        for (i = 0; i < 11; i++) {
            if (ptr[124 + i] >= '0' && ptr[124 + i] <= '7') size = size * 8 + (ptr[124 + i] - '0');
        }

        if (name[0] == 't' && name[5] == 'p') {
            png_load(ptr + 512, size);
            png_render(200, 150);
            return;
        }

        unsigned int blocks = (size + 511) / 512;
        ptr += 512 + blocks * 512;
    }
}

void desktop_draw(void) {
    for (int y = 0; y < 600; y++) {
        for (int x = 0; x < 800; x++) {
            fb_putpixel(x, y, 0xCCCCCC);
        }
    }
    for (int y = 0; y < 30; y++) {
        for (int x = 0; x < 800; x++) {
            fb_putpixel(x, y, 0x444444);
        }
    }
    draw_window(100, 100, 300, 200);
    // desktop_render_png();
}

static int desktop_init(void) {
    desktop_draw();
    return 0;
}

MODULE_EXPORT module_t desktop_module = {
    .magic = MODULE_MAGIC,
    .name = "desktop",
    .desc = "Desktop environment",
    .init = desktop_init,
    .next = NULL
};
