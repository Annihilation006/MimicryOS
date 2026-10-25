/* keyboard.c - 中断驱动键盘 */
#include "idt.h"
#include "console.h"
#include "pic.h"

#define KB_BUFFER_SIZE 256

static char kb_buffer[KB_BUFFER_SIZE];
static int kb_start = 0;
static int kb_end = 0;

static const char keymap[] = {
    0, 27, '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '-', '=', '\b',
    '\t', 'q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p', '[', ']', '\n',
    0, 'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', ';', '\'', '`',
    0, '\\', 'z', 'x', 'c', 'v', 'b', 'n', 'm', ',', '.', '/', 0,
    '*', 0, ' ', 0
};

static void keyboard_handler(interrupt_frame_t* frame) {
    unsigned char sc = inb(0x60);
    if (sc < 0x80 && sc < sizeof(keymap)) {
        char c = keymap[sc];
        if (c) {
            int next = (kb_end + 1) % KB_BUFFER_SIZE;
            if (next != kb_start) {
                kb_buffer[kb_end] = c;
                kb_end = next;
            }
        }
    }
}

void keyboard_init(void) {
    register_interrupt_handler(IRQ_BASE + 1, keyboard_handler);
    pic_clear_mask(1);
    kb_start = 0;
    kb_end = 0;
    print("Keyboard initialized.\n");
}

char keyboard_read(void) {
    while (kb_start == kb_end) {
        __asm__ volatile("hlt");
    }
    char c = kb_buffer[kb_start];
    kb_start = (kb_start + 1) % KB_BUFFER_SIZE;
    return c;
}