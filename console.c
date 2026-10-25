/* console.c - VGA控制台驱动 */
#include "console.h"

#define VIDEO_MEMORY 0xB8000
#define SCREEN_WIDTH 80
#define SCREEN_HEIGHT 25

static int cursor_x = 0;
static int cursor_y = 0;

static unsigned short* get_video_offset(int x, int y) {
    return (unsigned short*)(VIDEO_MEMORY + 2 * (y * SCREEN_WIDTH + x));
}

void clear_screen(void) {
    unsigned short blank = (0x0F << 8) | ' ';
    for (int y = 0; y < SCREEN_HEIGHT; y++) {
        for (int x = 0; x < SCREEN_WIDTH; x++) {
            *get_video_offset(x, y) = blank;
        }
    }
    cursor_x = 0;
    cursor_y = 0;
}

static void scroll_screen(void) {
    if (cursor_y >= SCREEN_HEIGHT) {
        for (int y = 0; y < SCREEN_HEIGHT - 1; y++) {
            for (int x = 0; x < SCREEN_WIDTH; x++) {
                *get_video_offset(x, y) = *get_video_offset(x, y + 1);
            }
        }
        unsigned short blank = (0x0F << 8) | ' ';
        for (int x = 0; x < SCREEN_WIDTH; x++) {
            *get_video_offset(x, SCREEN_HEIGHT - 1) = blank;
        }
        cursor_y = SCREEN_HEIGHT - 1;
    }
}

static void update_cursor(void) {
    unsigned short position = cursor_y * SCREEN_WIDTH + cursor_x;
    outb(0x3D4, 0x0F);
    outb(0x3D5, (unsigned char)(position & 0xFF));
    outb(0x3D4, 0x0E);
    outb(0x3D5, (unsigned char)((position >> 8) & 0xFF));
}

void putchar(char c) {
    unsigned short* video;
    
    switch (c) {
        case '\n':
            cursor_x = 0;
            cursor_y++;
            break;
        case '\r':
            cursor_x = 0;
            break;
        case '\t':
            cursor_x = (cursor_x + 8) & ~7;
            break;
        case '\b':
            if (cursor_x > 0) {
                cursor_x--;
                *get_video_offset(cursor_x, cursor_y) = (0x0F << 8) | ' ';
            }
            break;
        default:
            if (c >= ' ') {
                *get_video_offset(cursor_x, cursor_y) = (0x0F << 8) | c;
                cursor_x++;
            }
            break;
    }
    
    if (cursor_x >= SCREEN_WIDTH) {
        cursor_x = 0;
        cursor_y++;
    }
    
    scroll_screen();
    update_cursor();
}

void print(const char* str) {
    while (*str) putchar(*str++);
}

void print_int(int num) {
    char buffer[32];
    int i = 30;
    buffer[31] = '\0';
    
    if (num == 0) { putchar('0'); return; }
    if (num < 0) { putchar('-'); num = -num; }
    
    while (num > 0) {
        buffer[i--] = '0' + (num % 10);
        num /= 10;
    }
    print(&buffer[i + 1]);
}

void print_hex(unsigned int num) {
    char* hex = "0123456789ABCDEF";
    char buffer[16];
    int i = 14;
    buffer[15] = '\0';
    
    print("0x");
    if (num == 0) { print("0"); return; }
    
    while (num > 0 && i >= 0) {
        buffer[i--] = hex[num & 0xF];
        num >>= 4;
    }
    print(&buffer[i + 1]);
}

unsigned char inb(unsigned short port) {
    unsigned char result;
    __asm__ volatile("inb %1, %0" : "=a"(result) : "d"(port));
    return result;
}

void outb(unsigned short port, unsigned char data) {
    __asm__ volatile("outb %0, %1" : : "a"(data), "d"(port));
}