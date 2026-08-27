#ifndef CONSOLE_H
#define CONSOLE_H

void clear_screen(void);
void putchar(char c);
void print(const char* str);
void print_int(int num);
void print_hex(unsigned int num);
char read_keyboard(void);
unsigned char inb(unsigned short port);
void outb(unsigned short port, unsigned char data);

#endif