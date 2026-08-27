typedef unsigned long long uint64_t;
typedef unsigned int uint32_t;
typedef unsigned short uint16_t;
typedef unsigned char uint8_t;
typedef short int16_t;

#include "bootboot.h"

#define NULL ((void*)0)
void fb_print(const char* str);
void fb_putchar(int cx, int cy, char c, uint32_t color);

void ata_read(unsigned int lba, void* buffer);
void ata_write(unsigned int lba, void* buffer);
void outw(uint16_t port, uint16_t val);
void file_list_all(void);
void desktop_draw(void);
void file_create(const char* name);
void file_write(const char* name, const char* content);
void file_size(const char* name);
void file_delete(const char* name);
void file_cat(const char* name);
void fat12_format(void);
void fat12_create_file(const char* name, const char* content);
void fat12_list_dir(void);
void fat12_read_file(const char* name);
void fat12_delete_file(const char* name);
void fat12_create_full(const char* name, const char* content);
void fat12_format(void);

extern BOOTBOOT bootboot;
extern volatile unsigned char _binary_font_psf_start;

static uint64_t fb;
static int cursor_x = 0, cursor_y = 0;

#define MODULE_MAGIC 0x4D4F4455

typedef struct module {
    unsigned int magic;
    const char* name;
    const char* desc;
    int (*init)(void);
    struct module* next;
} module_t;

static module_t* module_head = NULL;

int module_register(module_t* mod) {
    if (mod->magic != MODULE_MAGIC) return -1;
    mod->next = module_head;
    module_head = mod;
    if (mod->init) mod->init();
    return 0;
}

void module_list(void) {
    module_t* m = module_head;
    while (m) {
        fb_print(m->name);
        fb_print(" - ");
        fb_print(m->desc);
        fb_print("\n");
        m = m->next;
    }
}

#define HEAP_START 0x200000
#define HEAP_SIZE  0x100000

typedef struct mem_block {
    unsigned int size;
    int free;
    struct mem_block* next;
} mem_block_t;

static mem_block_t* heap = NULL;

void memory_init(void) {
    heap = (mem_block_t*)HEAP_START;
    heap->size = HEAP_SIZE - sizeof(mem_block_t);
    heap->free = 1;
    heap->next = NULL;
}

void* kmalloc(unsigned int size) {
    mem_block_t* cur = heap;
    
    while (cur) {
        if (cur->free && cur->size >= size + sizeof(mem_block_t)) {
            mem_block_t* new_block = (mem_block_t*)((char*)cur + sizeof(mem_block_t) + size);
            new_block->size = cur->size - size - sizeof(mem_block_t);
            new_block->free = 1;

            new_block->next = cur->next;
            
            cur->size = size;
            cur->free = 0;
            cur->next = new_block;
            
            return (void*)((char*)cur + sizeof(mem_block_t));
        }
        cur = cur->next;
    }
    
    return NULL;
}

void kfree(void* ptr) {
    if (!ptr) return;
    mem_block_t* block = (mem_block_t*)((char*)ptr - sizeof(mem_block_t));
    block->free = 1;
}

int strcmp(const char* s1, const char* s2) {
    while (*s1 && *s2 && *s1 == *s2) {
        s1++;
        s2++;
    }
    return *s1 - *s2;
}

static char current_user[32] = "admin";

void user_login(const char* name) {
    int i = 0;
    while (name[i] && i < 31) {
        current_user[i] = name[i];
        i++;
    }
    current_user[i] = '\0';
}

void user_prompt(void) {
    int i = 0;
    while (current_user[i]) {
        fb_putchar(cursor_x, cursor_y, current_user[i], 0x00FF00);
        cursor_x++;
        i++;
    }
    fb_putchar(cursor_x, cursor_y, '#', 0xFF0000);
    cursor_x++;
    fb_print("MimicryOS> ");
}

static int user_module_init(void) {
    return 0;
}

extern module_t filesystem_module;
extern module_t mouse_module;
extern module_t desktop_module;
extern module_t png_module;
static module_t user_module = {
    .magic = MODULE_MAGIC,
    .name = "user",
    .desc = "User management",
    .init = user_module_init,
    .next = NULL
};

void scan_modules(void) {
    unsigned char* ptr = (unsigned char*)bootboot.initrd_ptr;
    unsigned char* end = ptr + bootboot.initrd_size;
    
    fb_print("  [");
    while (ptr < end) {
        char name[101];
        int i;
        for (i = 0; i < 100 && ptr[i]; i++) {
            name[i] = ptr[i];
        }
        name[i] = '\0';
        
        if (name[0] == '\0') break;
        
        int is_registry = 1;
        const char* target = "module/registry/admin/registry";
        for (i = 0; target[i]; i++) {
            if (name[i] != target[i]) {
                is_registry = 0;
                break;
            }
        }
        
        unsigned int size = 0;
        for (i = 0; i < 11; i++) {
            if (ptr[124 + i] >= '0' && ptr[124 + i] <= '7') {
                size = size * 8 + (ptr[124 + i] - '0');
            }
        }
        
        if (is_registry) {
            unsigned char* content = ptr + 512;
            
            int pos = 0;
            for (unsigned int j = 0; j < size; j++) {
                char c = content[j];
                if (c == '\n') {
                    if (pos > 0) {
                        if (content[j - pos] != '*' && content[j - pos] != '[') {
                            for (int k = 0; k < pos; k++) {
                                char s[2];
                                s[0] = content[j - pos + k];
                                s[1] = '\0';
                                fb_print(s);
                            }
                            fb_print(" ");
        for (int j = 8; j < 11; j++) {
}
                        }
                    }
                    pos = 0;
                } else {
                    pos++;
                }
            }
            break;
        }
        
        unsigned int blocks = (size + 511) / 512;
        ptr += 512 + blocks * 512;
    }
    fb_print("]\n");
}


typedef struct {
    uint32_t magic;
    uint32_t version;
    uint32_t headersize;
    uint32_t flags;
    uint32_t numglyph;
    uint32_t bytesperglyph;
    uint32_t height;
    uint32_t width;
    uint8_t glyphs;
} __attribute__((packed)) psf2_t;

uint8_t inb(uint16_t port) {
    uint8_t r;
    __asm__ volatile("inb %1, %0" : "=a"(r) : "d"(port));
    return r;
}


void outb(uint16_t port, uint8_t val) {
    __asm__ volatile("outb %0, %1" : : "a"(val), "d"(port));
}

unsigned short inw(unsigned short port) {
    unsigned short r;
    __asm__ volatile("inw %1, %0" : "=a"(r) : "d"(port));
    return r;
}

void fat12_format(void) {
    unsigned char boot[512];
    for (int i = 0; i < 512; i++) boot[i] = 0;
    boot[0] = 0xEB; boot[1] = 0x3C; boot[2] = 0x90;
    boot[3] = 'M'; boot[4] = 'I'; boot[5] = 'M'; boot[6] = 'I';
    boot[7] = 'C'; boot[8] = 'R'; boot[9] = 'Y'; boot[10] = 'O'; boot[11] = 'S';
    boot[11] = 0x00; boot[12] = 0x02;
    boot[13] = 0x01;
    boot[14] = 0x01; boot[15] = 0x00;
    boot[16] = 0x02;
    boot[17] = 0xE0; boot[18] = 0x00;
    boot[19] = 0x40; boot[20] = 0x0B;
    boot[21] = 0xF0;
    boot[22] = 0x09; boot[23] = 0x00;
    boot[24] = 0x12; boot[25] = 0x00;
    boot[26] = 0x02; boot[27] = 0x00;
    boot[510] = 0x55; boot[511] = 0xAA;
    ata_write(2880, boot);
    fb_print("FAT12 formatted!\n");
}

void fat12_create_file(const char* name, const char* content) {
void fat12_list_dir(void);
void fat12_read_file(const char* name);
void fat12_delete_file(const char* name);
void fat12_create_full(const char* name, const char* content);
    unsigned char dir[512];
    ata_read(2885, dir);
    int entry = -1;
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00 || dir[i] == 0xE5) {
            entry = i;
            break;
        }
    }
    if (entry < 0) {
        fb_print("No free entry\n");
        return;
    }
    int i;
    for (i = 0; i < 8 && name[i] && name[i] != '.'; i++) {
        dir[entry + i] = name[i];
    }
    for (; i < 8; i++) dir[entry + i] = ' ';
    for (i = 8; i < 11; i++) dir[entry + i] = ' ';
    dir[entry + 11] = 0x20;
    dir[entry + 26] = 0x02;
    dir[entry + 27] = 0x00;
    int len = 0;
    while (content[len]) len++;
    dir[entry + 28] = len & 0xFF;
    dir[entry + 29] = (len >> 8) & 0xFF;
    ata_write(2885, dir);
    fb_print("File created\n");
}

void fat12_list_dir(void) {
    unsigned char dir[512];
    ata_read(2885, dir);
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00) break;
        if (dir[i] == 0xE5) continue;
        for (int j = 0; j < 8; j++) {
            char c = dir[i + j];
            if (c >= ' ' && c <= '~') {
                char s[2];
                s[0] = c;
                s[1] = '\0';
                fb_print(s);
            }
        }
        fb_print(".");
        for (int j = 8; j < 11; j++) {
            char c = dir[i + j];
            if (c >= ' ' && c <= '~') {
                char s[2];
                s[0] = c;
                s[1] = '\0';
                fb_print(s);
            }
        fb_print("\n");
    }
}
        }
void fat12_read_file(const char* name) {
    unsigned char dir[512];
    ata_read(2885, dir);
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00) break;
        if (dir[i] == 0xE5) continue;
        int match = 1;
        for (int j = 0; j < 8 && name[j]; j++) {
            if (dir[i + j] != name[j]) {
                match = 0;
                break;
            }
        }
        if (match) {
            unsigned char data[512];
            ata_read(2890, data);
            fb_print(data);
            fb_print("\n");
            return;
        }
    }
    fb_print("File not found\n");
}

void fat12_create_full(const char* name, const char* content) {
    unsigned char dir[512];
    ata_read(2885, dir);
    int entry = -1;
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00 || dir[i] == 0xE5) {
            entry = i;
            break;
        }
    }
    if (entry < 0) { fb_print("No free entry\n"); return; }
    int i;
    for (i = 0; i < 8 && name[i] && name[i] != '.'; i++) dir[entry + i] = name[i];
    for (; i < 8; i++) dir[entry + i] = ' ';
    for (i = 0; i < 3; i++) {
        int k = 0;
        while (name[k] && name[k] != '.') k++;
        if (name[k] == '.') {
            dir[entry + 8 + i] = name[k + 1 + i];
        } else {
            dir[entry + 8 + i] = ' ';
        }
    }
    dir[entry + 11] = 0x20;
    dir[entry + 26] = 0x02;
    dir[entry + 27] = 0x00;
    int len = 0;
    while (content[len]) len++;
    dir[entry + 28] = len & 0xFF;
    dir[entry + 29] = (len >> 8) & 0xFF;
    ata_write(2885, dir);
    if (len > 0) {
        unsigned char data[512];
        for (int j = 0; j < 512; j++) data[j] = 0;
        for (int j = 0; j < len && j < 512; j++) data[j] = content[j];
        ata_write(2890, data);
    }
    fb_print("File created\n");
}


void fat12_delete_file(const char* name) {
    unsigned char dir[512];
    ata_read(2885, dir);
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00) break;
        if (dir[i] == 0xE5) continue;
        int match = 1;
        for (int j = 0; j < 8 && name[j]; j++) {
            if (dir[i + j] != name[j]) {
                match = 0;
                break;
            }
        }
        if (match) {
            dir[i] = 0xE5;
            ata_write(2885, dir);
            fb_print("Deleted\n");
            return;
        }
    }
    fb_print("File not found\n");
}

void ata_write(unsigned int lba, void* buffer) {
    outb(0x1F6, 0xE0);
    outb(0x1F2, 1);
    outb(0x1F3, lba & 0xFF);
    outb(0x1F4, (lba >> 8) & 0xFF);
    outb(0x1F5, (lba >> 16) & 0xFF);
    outb(0x1F7, 0x30);
    while (!(inb(0x1F7) & 0x08));
    for (int i = 0; i < 256; i++) {
        outw(0x1F0, ((unsigned short*)buffer)[i]);
    }
}

void ata_read(unsigned int lba, void* buffer) {
    outb(0x1F6, 0xE0);
    outb(0x1F2, 1);
    outb(0x1F3, lba & 0xFF);
    outb(0x1F4, (lba >> 8) & 0xFF);
    outb(0x1F5, (lba >> 16) & 0xFF);
    outb(0x1F7, 0x20);
    while (!(inb(0x1F7) & 0x08));
    for (int i = 0; i < 256; i++) {
        ((unsigned short*)buffer)[i] = inw(0x1F0);
    }
}
void outw(uint16_t port, uint16_t val) {
    __asm__ volatile("outw %0, %1" : : "a"(val), "d"(port));
}

void fb_putpixel(int x, int y, uint32_t color) {
    *((uint32_t*)(fb + bootboot.fb_scanline * y + x * 4)) = color;
}

void fb_fill(uint32_t color) {
    for (int y = 0; y < (int)bootboot.fb_height; y++) {
        for (int x = 0; x < (int)bootboot.fb_width; x++) {
            fb_putpixel(x, y, color);
        }
    }
}

void fb_putchar(int cx, int cy, char c, uint32_t color) {
    psf2_t* font = (psf2_t*)&_binary_font_psf_start;
    int bytesperline = (font->width + 7) / 8;
    unsigned char* glyph = (unsigned char*)&_binary_font_psf_start +
        font->headersize + (c > 0 && c < (int)font->numglyph ? c : 0) * font->bytesperglyph;
    
    for (int y = 0; y < (int)font->height; y++) {
        int line = 0;
        int mask = 1 << (font->width - 1);
        for (int x = 0; x < (int)font->width; x++) {
            if (glyph[y * bytesperline + line] & mask) {
                fb_putpixel(cx * (font->width + 1) + x, cy * (font->height + 1) + y, color);
            }
            mask >>= 1;
            if (mask == 0) {
                mask = 1 << 7;
                line++;
            }
        }
    }
}

void fb_print(const char* str) {
    int i = 0;
    while (str[i]) {
        if (str[i] == '\n') {
            cursor_x = 0;
            cursor_y++;
        } else {
            fb_putchar(cursor_x, cursor_y, str[i], 0xFFFFFF);
            cursor_x++;
        }
        i++;
    }
}

char keyboard_read(void) {
    static int shift = 0;
    
    while (1) {
        uint8_t kstatus = inb(0x64);
        if (kstatus & 0x01) {
            if (kstatus & 0x20) { inb(0x60); continue; }
            uint8_t sc = inb(0x60);
            
            if (sc == 0x55) continue;
            if (sc == 0x5B || sc == 0x5C) { inb(0x60); continue; }
            
            if (sc == 0x2A || sc == 0x36) {
                shift = 1;
                continue;
            }
            if (sc == 0xAA || sc == 0xB6) {
                shift = 0;
                continue;
            }
            
            if (sc < 0x80) {
                const char keymap[] = {
                    0, 27, '1','2','3','4','5','6','7','8','9','0','-','=','\b',
                    '\t','q','w','e','r','t','y','u','i','o','p','[',']','\n',
                    0,'a','s','d','f','g','h','j','k','l',';','\'','`',
                    0,'\\','z','x','c','v','b','n','m',',','.','/',0,
                    '*',0,' ',0
                };
                const char keymap_shift[] = {
                    0, 27, '!','@','#','$','%','^','&','*','(',')','_','+','\b',
                    '\t','Q','W','E','R','T','Y','U','I','O','P','{','}','\n',
                    0,'A','S','D','F','G','H','J','K','L',':','"','~',
                    0,'|','Z','X','C','V','B','N','M','<','>','?',0,
                    '*',0,' ',0
                };
                
                if (sc < sizeof(keymap)) {
                    if (shift) return keymap_shift[sc];
                    else return keymap[sc];
                }
            }
        }
        for (volatile int i = 0; i < 50000; i++);
    }
}

void shell(void) {
    cursor_x = 0;
    cursor_y = 3;
    user_prompt();

    char cmd[256];
    int pos = 0;

    while (1) {
        for (int i = 0; i < 8; i++) {
            fb_putpixel(cursor_x * 9 + i, cursor_y * 17 + 15, 0xFFFFFF);
            fb_putpixel(cursor_x * 9 + i, cursor_y * 17 + 16, 0xFFFFFF);
        }
        
        char c = keyboard_read();
        
        for (int i = 0; i < 8; i++) {
            fb_putpixel(cursor_x * 9 + i, cursor_y * 17 + 15, 0x000000);
            fb_putpixel(cursor_x * 9 + i, cursor_y * 17 + 16, 0x000000);
        }
        
        if (c == '\n') {
            cmd[pos] = '\0';
            
            if (cursor_y >= 40) {
                fb_fill(0x000000);
                cursor_x = 0;
                cursor_y = 0;
                fb_print("MimicryOS Shell\n");
            }
            
            fb_print("\n");
            
            if (pos == 0) {
                user_prompt();
            } else if (cmd[0] == 'h' && cmd[1] == 'e' && cmd[2] == 'l' && cmd[3] == 'p' && cmd[4] == '\0') {
                user_prompt();
            } else if (cmd[0] == 'c' && cmd[1] == 'l' && cmd[2] == 'e' && cmd[3] == 'a' && cmd[4] == 'r' && cmd[5] == '\0') {
                fb_fill(0x000000);
                cursor_x = 0;
                cursor_y = 0;
                fb_print("MimicryOS Shell\n");
                user_prompt();
            } else if (cmd[0] == 'a' && cmd[1] == 'b' && cmd[2] == 'o' && cmd[3] == 'u' && cmd[4] == 't' && cmd[5] == '\0') {
                fb_print("MimicryOS - a simple OS\n");
                user_prompt();
            } else if (cmd[0] == 'm' && cmd[1] == 'o' && cmd[2] == 'd' && cmd[3] == 'u' && cmd[4] == 'l' && cmd[5] == 'e' && cmd[6] == 's' && cmd[7] == '\0') {
                module_list();
                user_prompt();
            } else if (cmd[0] == 'm' && cmd[1] == 'e' && cmd[2] == 'm' && cmd[3] == 'o' && cmd[4] == 'r' && cmd[5] == 'y' && cmd[6] == '\0') {
                fb_print("Memory: heap initialized\n");
                user_prompt();
            } else if (cmd[0] == 's' && cmd[1] == 'c' && cmd[2] == 'a' && cmd[3] == 'n' && cmd[4] == '\0') {
                scan_modules();
                user_prompt();
            } else if (cmd[0] == 's' && cmd[1] == 'y' && cmd[2] == 's' && cmd[3] == 'i' && cmd[4] == 'n' && cmd[5] == 'f' && cmd[6] == 'o' && cmd[7] == '\0') {
                fb_print("MimicryOS v0.1\n");
                fb_print("64-bit kernel\n");
                fb_print("BOOTBOOT loader\n");
                user_prompt();
            } else if (cmd[0] == 'l' && cmd[1] == 's' && cmd[2] == '\0') {
                file_list_all();
                user_prompt();
            } else if (cmd[0] == 'd' && cmd[1] == 'e' && cmd[2] == 's' && cmd[3] == 'k' && cmd[4] == 't' && cmd[5] == 'o' && cmd[6] == 'p' && cmd[7] == '\0') {
                desktop_draw();
                keyboard_read();
                fb_fill(0x000000);
                cursor_x = 0;
                cursor_y = 0;
                fb_print("MimicryOS Shell\n");
                user_prompt();
            } else if (cmd[0] == 'm' && cmd[1] == 'k' && cmd[2] == 'f' && cmd[3] == 'i' && cmd[4] == 'l' && cmd[5] == 'e' && cmd[6] == ' ') {
                fat12_create_full(&cmd[7], "");
                user_prompt();
            } else if (cmd[0] == 'f' && cmd[1] == 'o' && cmd[2] == 'r' && cmd[3] == 'm' && cmd[4] == 'a' && cmd[5] == 't' && cmd[6] == '\0') {
                fat12_format();
                user_prompt();
            } else if (cmd[0] == 'c' && cmd[1] == 'a' && cmd[2] == 't' && cmd[3] == ' ') {
                fat12_read_file(&cmd[4]);
                user_prompt();
            } else if (cmd[0] == 'd' && cmd[1] == 'e' && cmd[2] == 'l' && cmd[3] == ' ') {
                fat12_delete_file(&cmd[4]);
                user_prompt();
            } else if (cmd[0] == 'd' && cmd[1] == 'i' && cmd[2] == 'r' && cmd[3] == '\0') {
                fat12_list_dir();
                user_prompt();
            } else if (cmd[0] == 'd' && cmd[1] == 'i' && cmd[2] == 's' && cmd[3] == 'k' && cmd[4] == '\0') {
                char buf[512];
                ata_read(0, buf);
                fb_print("Disk read OK\n");
                user_prompt();
            } else if (cmd[0] == 'd' && cmd[1] == 'a' && cmd[2] == 't' && cmd[3] == 'e' && cmd[4] == '\0') {
                fb_print("2025-08-25\n");
                user_prompt();
            } else if (cmd[0] == 'e' && cmd[1] == 'c' && cmd[2] == 'h' && cmd[3] == 'o' && cmd[4] == ' ') {
                fb_print(&cmd[5]);
                fb_print("\n");
                user_prompt();
            } else if (cmd[0] == 'w' && cmd[1] == 'h' && cmd[2] == 'o' && cmd[3] == 'a' && cmd[4] == 'm' && cmd[5] == 'i' && cmd[6] == '\0') {
                fb_print(current_user);
                fb_print("\n");
                user_prompt();
            } else if (cmd[0] == 't' && cmd[1] == 'o' && cmd[2] == 'u' && cmd[3] == 'c' && cmd[4] == 'h' && cmd[5] == ' ') {
                file_create(&cmd[6]);
                user_prompt();
            } else if (cmd[0] == 'w' && cmd[1] == 'r' && cmd[2] == 'i' && cmd[3] == 't' && cmd[4] == 'e' && cmd[5] == ' ') {
                char fname[32];
                int fi = 6, fj = 0;
                while (cmd[fi] != ' ' && cmd[fi] && fj < 31) {
                    fname[fj++] = cmd[fi++];
                }
                fname[fj] = '\0';
                if (cmd[fi] == ' ') fi++;
                file_write(fname, &cmd[fi]);
                user_prompt();
            } else if (cmd[0] == 's' && cmd[1] == 'i' && cmd[2] == 'z' && cmd[3] == 'e' && cmd[4] == ' ') {
                file_size(&cmd[5]);
                user_prompt();
            } else if (cmd[0] == 'r' && cmd[1] == 'm' && cmd[2] == ' ') {
                file_delete(&cmd[3]);
                user_prompt();
            } else if (cmd[0] == 'c' && cmd[1] == 'a' && cmd[2] == 't' && cmd[3] == ' ') {
                file_cat(&cmd[4]);
                user_prompt();
            } else if (cmd[0] == 'r' && cmd[1] == 'e' && cmd[2] == 'b' && cmd[3] == 'o' && cmd[4] == 'o' && cmd[5] == 't' && cmd[6] == '\0') {
                outb(0x64, 0xFE);
            } else if (cmd[0] == 's' && cmd[1] == 'h' && cmd[2] == 'u' && cmd[3] == 't' && cmd[4] == 'd' && cmd[5] == 'o' && cmd[6] == 'w' && cmd[7] == 'n' && cmd[8] == '\0') {
                outw(0x604, 0x2000);
                for (volatile int si = 0; si < 100000; si++);
                outw(0x5301, 0x0000);
                outw(0x530E, 0x0000);
                outw(0x5307, 0x0001);
                for (volatile int si = 0; si < 100000; si++);
                outw(0x5301, 0x0000);
                outw(0x530E, 0x0000);
                outw(0x5307, 0x0001);
            } else {
                fb_print("Unknown command: ");
                fb_print(cmd);
                fb_print("\n");
                user_prompt();
            }
            pos = 0;
        } else if (c == '\b') {
            if (pos > 0) {
                pos--;
                cursor_x--;
                for (int y = 0; y < 17; y++) {
                    for (int x = 0; x < 9; x++) {
                        fb_putpixel(cursor_x * 9 + x, cursor_y * 17 + y, 0x000000);
                    }
                }
            }
        } else if (c >= ' ' && pos < 255) {
            for (int y = 0; y < 17; y++) {
                for (int x = 0; x < 9; x++) {
                    fb_putpixel(cursor_x * 9 + x, cursor_y * 17 + y, 0x000000);
                }
            }
            cmd[pos++] = c;
            fb_putchar(cursor_x, cursor_y, c, 0xFFFFFF);
            cursor_x++;
        }
    }
}

void boot_animation(void) {
    fb_fill(0x000000);
    cursor_x = 0;
    cursor_y = 2;
    fb_print("  Starting MimicryOS...\n");
    fb_print("\n");
    fb_print("  Loading modules...\n");
//    scan_modules();
    for (volatile int i = 0; i < 1000000; i++);
    fb_fill(0x000000);
    cursor_x = 0;
    cursor_y = 0;
}

void kernel_main(void) {
    fb = bootboot.fb_ptr;
    fb_fill(0x000000);
    
    memory_init();
    module_register(&filesystem_module);
    module_register(&mouse_module);
    // module_register(&desktop_module);
    module_register(&png_module);
    module_register(&user_module);
    
    boot_animation();
    fb_print("MimicryOS Shell\n");
    fb_print("Type help for commands\n");
    
    shell();
}

__attribute__((used)) void _start(void) {
    __asm__ volatile("cli");
    __asm__ volatile("mov $0x9F000, %rsp");
    kernel_main();
}
