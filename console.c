/* console.c — print/print_hex 的薄壳实现
 * 原版直写 0xB8000 VGA 文本显存, 但 BOOTBOOT 是图形模式线性帧缓冲,
 * 0xB8000 在这没有映射, 所以统一走 kernel.c 的 fb_print */
#include "console.h"

extern void fb_print(const char* str);

void clear_screen(void) {
    fb_print("\n");
}

void print(const char* str) {
    fb_print(str);
}

void print_int(int num) {
    char buf[16];
    int i = 14;
    buf[15] = '\0';
    if (num == 0) { fb_print("0"); return; }
    if (num < 0) { fb_print("-"); num = -num; }
    while (num > 0 && i >= 0) { buf[i--] = '0' + num % 10; num /= 10; }
    fb_print(&buf[i + 1]);
}

void print_hex(unsigned int num) {
    char* hex = "0123456789ABCDEF";
    char buf[12];
    int i = 10;
    buf[11] = '\0';
    fb_print("0x");
    if (num == 0) { fb_print("0"); return; }
    while (num > 0 && i >= 0) { buf[i--] = hex[num & 0xF]; num >>= 4; }
    fb_print(&buf[i + 1]);
}
