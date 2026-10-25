/* ============================================================
 * MimicryOS 64 位内核主文件
 * - 由 BOOTBOOT 二级引导加载器在长模式下加载，入口为 _start（见文件末尾）
 * - 功能：内存堆管理、ATA 硬盘读写、FAT12 风格文件系统、VGA framebuffer
 *   文本渲染、PS/2 键盘轮询、命令行 Shell、模块注册
 * - 磁盘布局（自制简化版，非标准 FAT12）：
 *     LBA 2880       引导扇区
 *     LBA 2881-2884  FAT 区（当前保留未使用）
 *     LBA 2885-2889  根目录区（每项 32 字节，最多 16 项）
 *     LBA 2890       数据区（当前固定单扇区，尚未实现多簇链）
 * - 说明：中断系统(IDT/int 0x80)、多任务、用户态(Ring3)尚未实现，
 *   属于后续阶段目标。
 * ============================================================ */
typedef unsigned long long uint64_t;
typedef unsigned int uint32_t;
typedef unsigned short uint16_t;
typedef unsigned char uint8_t;
typedef short int16_t;

#include "bootboot.h"
#include "idt.h"
#include "pic.h"
#include "console.h"

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

/* ---- 模块系统：单向链表注册 + 列表查询 ----
 * 每个模块提供一个 module_t，用 module_register 挂入链表并调用其 init()。 */
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

#define HEAP_SIZE  0x100000

typedef struct mem_block {
    unsigned int size;
    int free;
    struct mem_block* next;
} mem_block_t;

/* 堆放在内核 .bss 段内（链接器分配的虚拟地址，保证可写），
 * 避免原 0x200000 裸物理地址在长模式页表下可能未映射而触发 page fault。 */
static unsigned char heap_region[HEAP_SIZE] __attribute__((aligned(8)));
static mem_block_t* heap = NULL;

void memory_init(void) {
    heap = (mem_block_t*)heap_region;
    heap->size = HEAP_SIZE - sizeof(mem_block_t);
    heap->free = 1;
    heap->next = NULL;
}

/* 简单堆分配器：first-fit 遍历空闲链表，找到足够大的块就从中分裂出
 * 一个新区块（块头 mem_block_t 记录 size/free），返回块头之后的用户
 * 数据指针。用 kmalloc_size() 可反查某次分配的实际块大小。 */
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
    /* 合并相邻空闲块，减少碎片化 */
    mem_block_t* cur = heap;
    while (cur) {
        if (cur->free && cur->next && cur->next->free) {
            char* end_cur = (char*)cur + sizeof(mem_block_t) + cur->size;
            if (end_cur == (char*)cur->next) {
                cur->size = cur->size + sizeof(mem_block_t) + cur->next->size;
                cur->next = cur->next->next;
                continue;   /* 继续尝试与更后面的块合并 */
            }
        }
        cur = cur->next;
    }
}

/* 返回某次 kmalloc 分配的实际字节数（供正确的 realloc 使用） */
unsigned int kmalloc_size(void* ptr) {
    if (!ptr) return 0;
    mem_block_t* block = (mem_block_t*)((char*)ptr - sizeof(mem_block_t));
    return block->size;
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
    /* BOOTBOOT initrd 的真实格式由引导加载器打包决定，原实现用臆造的
     * 偏移（124 字节 8 进制大小）解析 registry，会越界读内存导致崩溃。
     * 此处保守处理：只报告 initrd 是否存在，跳过 registry 扫描。 */
    if (bootboot.initrd_ptr && bootboot.initrd_size > 0) {
        fb_print("  [initrd present, registry scan skipped]\n");
    } else {
        fb_print("  [no initrd]\n");
    }
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
    boot[7] = 'C'; boot[8] = 'R'; boot[9] = 'Y'; boot[10] = 'O';
    boot[11] = 0x00; boot[12] = 0x02;   /* 每扇区字节数 = 0x0200 */
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
    /* 清空 FAT 区（LBA 2881-2884）与根目录区（LBA 2885-2889），
     * 避免格式化后目录区读到磁盘残留垃圾 */
    unsigned char empty[512];
    for (int i = 0; i < 512; i++) empty[i] = 0;
    for (int l = 2881; l <= 2889; l++) ata_write(l, empty);
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

/* 8.3 文件名匹配：把用户输入(如 "test.txt")按「名+扩展」与目录项比较，忽略填充空格 */
static int fat12_match(const unsigned char* dir, int entry, const char* name) {
    int ni = 0, di;
    for (di = 0; di < 8; di++) {
        char nc = ' ';
        if (name[ni] && name[ni] != '.') nc = name[ni++];
        if (dir[entry + di] != nc) return 0;
    }
    if (name[ni] == '.') ni++;
    for (di = 0; di < 3; di++) {
        char nc = (name[ni]) ? name[ni++] : ' ';
        if (dir[entry + 8 + di] != nc) return 0;
    }
    return 1;
}

void fat12_list_dir(void) {
    unsigned char dir[512];
    ata_read(2885, dir);
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00) break;
        if (dir[i] == 0xE5) continue;
        int nl = 8;
        while (nl > 0 && dir[i + nl - 1] == ' ') nl--;   /* 名区去尾部填充空格 */
        for (int j = 0; j < nl; j++) {
            char s[2]; s[0] = dir[i + j]; s[1] = '\0'; fb_print(s);
        }
        int el = 3;
        while (el > 0 && dir[i + 8 + el - 1] == ' ') el--; /* 扩展区去尾部填充空格 */
        if (el > 0) {
            fb_print(".");
            for (int j = 0; j < el; j++) {
                char s[2]; s[0] = dir[i + 8 + j]; s[1] = '\0'; fb_print(s);
            }
        }
        fb_print("\n");
    }
}
void fat12_read_file(const char* name) {
    unsigned char dir[512];
    ata_read(2885, dir);
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00) break;
        if (dir[i] == 0xE5) continue;
        if (fat12_match(dir, i, name)) {
            unsigned char data[512];
            ata_read(2890, data);
            fb_print((const char*)data);
            fb_print("\n");
            return;
        }
    }
    fb_print("File not found\n");
}

void fat12_size(const char* name) {
    unsigned char dir[512];
    ata_read(2885, dir);
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00) break;
        if (dir[i] == 0xE5) continue;
        if (fat12_match(dir, i, name)) {
            int len = dir[i + 28] | (dir[i + 29] << 8);
            char buf[16];
            int n = len, bi = 0;
            if (n == 0) buf[bi++] = '0';
            while (n > 0) { buf[bi++] = '0' + (n % 10); n /= 10; }
            while (bi > 0) { char s[2]; s[0] = buf[--bi]; s[1] = '\0'; fb_print(s); }
            fb_print(" bytes\n");
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

void fat12_write_file(const char* name, const char* content) {
    unsigned char dir[512];
    ata_read(2885, dir);
    int entry = -1;
    for (int i = 0; i < 512; i += 32) {
        if (dir[i] == 0x00) break;
        if (dir[i] == 0xE5) continue;
        if (fat12_match(dir, i, name)) { entry = i; break; }
    }
    if (entry < 0) {                 /* 文件不存在：走创建逻辑 */
        fat12_create_full(name, content);
        return;
    }
    int len = 0;
    while (content[len]) len++;
    dir[entry + 28] = len & 0xFF;    /* 更新文件大小 */
    dir[entry + 29] = (len >> 8) & 0xFF;
    ata_write(2885, dir);
    if (len > 0) {                   /* 写内容到固定数据扇区 */
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
        if (fat12_match(dir, i, name)) {
            dir[i] = 0xE5;
            ata_write(2885, dir);
            fb_print("Deleted\n");
            return;
        }
    }
    fb_print("File not found\n");
}

/* ---- ATA 底层：加超时与错误检测，避免硬盘忙/错时死循环挂死 ---- */
static int ata_wait_not_busy(void) {
    for (int t = 0; t < 100000; t++) {
        uint8_t s = inb(0x1F7);
        if (!(s & 0x80)) return 1;        /* BSY 已清除 */
        for (volatile int d = 0; d < 1000; d++);
    }
    return 0;                             /* 超时 */
}
static int ata_wait_drq(void) {
    for (int t = 0; t < 100000; t++) {
        uint8_t s = inb(0x1F7);
        if (s & 0x01) return 0;           /* 错误位置位 */
        if (s & 0x08) return 1;           /* DRQ 就绪 */
        for (volatile int d = 0; d < 1000; d++);
    }
    return 0;                             /* 超时 */
}
static void ata_select(unsigned int lba) {
    outb(0x1F6, 0xE0 | ((lba >> 24) & 0x0F));  /* LBA 模式 + master + LBA 高 4 位 */
    outb(0x1F2, 1);                            /* 1 个扇区 */
    outb(0x1F3, lba & 0xFF);
    outb(0x1F4, (lba >> 8) & 0xFF);
    outb(0x1F5, (lba >> 16) & 0xFF);
}

void ata_write(unsigned int lba, void* buffer) {
    if (!ata_wait_not_busy()) { fb_print("ATA busy\n"); return; }
    ata_select(lba);
    outb(0x1F7, 0x30);                   /* 写扇区命令 */
    if (!ata_wait_drq()) { fb_print("ATA write fail\n"); return; }
    for (int i = 0; i < 256; i++) {
        outw(0x1F0, ((unsigned short*)buffer)[i]);
    }
}

void ata_read(unsigned int lba, void* buffer) {
    if (!ata_wait_not_busy()) { fb_print("ATA busy\n"); return; }
    ata_select(lba);
    outb(0x1F7, 0x20);                   /* 读扇区命令 */
    if (!ata_wait_drq()) { fb_print("ATA read fail\n"); return; }
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

/* 在字符格 (cx,cy) 绘制一个字符：从 PSF2 字体取 8x16 字形位图逐位描点。
 * 字宽 8、字高 16，字符格 9x17（含 1px 间距）。 */
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

/* PS/2 键盘轮询：读 0x64 状态端口，有数据则从 0x60 取扫描码。
 * 处理 Shift 组合键与扩展码(0xE0)前缀的方向键；
 * 方向键映射为 0x01上/0x02下/0x03左/0x04右，供 shell 使用。 */
char keyboard_read(void) {
    static int shift = 0;
    static int caps = 0;   /* CapsLock 状态：make(0x3A) 切换，break(0xBA) 忽略 */
    static int e0 = 0;
    
    while (1) {
        uint8_t kstatus = inb(0x64);
        if (kstatus & 0x01) {
            /* 数据可读（0x20 位是鼠标数据，与键盘无关，键盘统一从 0x60 读） */
            uint8_t sc = inb(0x60);
            
            /* 扩展码前缀：方向键/编辑键以 0xE0（个别 0xE1）开头 */
            if (sc == 0xE0 || sc == 0xE1) { e0 = 1; continue; }
            
            if (sc == 0x55) continue;                  /* 未知键 */
            if (sc == 0x5B || sc == 0x5C) continue;    /* 左/右 Win 键 make */
            if (sc == 0xDB || sc == 0xDC) continue;    /* 左/右 Win 键 break */
            
            if (sc == 0x2A || sc == 0x36) {           /* Shift make */
                shift = 1;
                continue;
            }
            if (sc == 0xAA || sc == 0xB6) {           /* Shift break */
                shift = 0;
                continue;
            }
            if (sc == 0x3A) { caps = !caps; continue; }  /* CapsLock make：切换大小写 */
            if (sc == 0xBA) continue;                    /* CapsLock break：忽略 */
            
            /* 扩展码后的方向键映射（返回非打印控制值，供 shell 使用） */
            if (e0) {
                e0 = 0;
                if (sc == 0x48 || sc == 0x47) return 0x01;  /* 上 / Home */
                if (sc == 0x50 || sc == 0x53) return 0x02;  /* 下 / End */
                if (sc == 0x4B)               return 0x03;  /* 左 */
                if (sc == 0x4D)               return 0x04;  /* 右 */
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
                    /* 大小写：Shift 与 CapsLock 异或（Shift^Caps=1 输出大写） */
                    if (shift ^ caps) return keymap_shift[sc];
                    else return keymap[sc];
                }
            }
        }
        for (volatile int i = 0; i < 50000; i++);
    }
}

/* 命令行 Shell：循环读键，支持左右方向键移动光标、中间插入、退格删除。
 * 按回车解析命令（分支见下），内置命令：help/clear/about/modules/memory/
 * scan/sysinfo/ls/desktop/mkfile/format/cat/del/dir/disk/date/echo/whoami/
 * touch/write/size/rm/type/reboot/shutdown。 */
void shell(void) {
    cursor_x = 0;
    /* [test] 原来写死 cursor_y=3, 横幅多打几行就叠字, 改成跟光标走 */
    user_prompt();

    char cmd[256];
    int pos = 0;          /* 已输入字符数 */
    int caret = 0;        /* 光标在命令中的位置（0..pos） */
    int prompt_col = cursor_x;   /* 提示符后的起始列 */

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
                fb_print("Commands: clear about modules memory scan sysinfo\n");
                fb_print("  ls desktop mkfile format cat del dir disk\n");
                fb_print("  date echo whoami touch write size rm type\n");
                fb_print("  reboot shutdown\n");
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
                fat12_list_dir();
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
                fat12_create_full(&cmd[6], "");
                user_prompt();
            } else if (cmd[0] == 'w' && cmd[1] == 'r' && cmd[2] == 'i' && cmd[3] == 't' && cmd[4] == 'e' && cmd[5] == ' ') {
                char fname[32];
                int fi = 6, fj = 0;
                while (cmd[fi] != ' ' && cmd[fi] && fj < 31) {
                    fname[fj++] = cmd[fi++];
                }
                fname[fj] = '\0';
                if (cmd[fi] == ' ') fi++;
                fat12_write_file(fname, &cmd[fi]);
                user_prompt();
            } else if (cmd[0] == 's' && cmd[1] == 'i' && cmd[2] == 'z' && cmd[3] == 'e' && cmd[4] == ' ') {
                fat12_size(&cmd[5]);
                user_prompt();
            } else if (cmd[0] == 'r' && cmd[1] == 'm' && cmd[2] == ' ') {
                fat12_delete_file(&cmd[3]);
                user_prompt();
            } else if (cmd[0] == 't' && cmd[1] == 'y' && cmd[2] == 'p' && cmd[3] == 'e' && cmd[4] == ' ') {
                fat12_read_file(&cmd[5]);
                user_prompt();
            } else if (cmd[0] == 'r' && cmd[1] == 'e' && cmd[2] == 'b' && cmd[3] == 'o' && cmd[4] == 'o' && cmd[5] == 't' && cmd[6] == '\0') {
                outb(0x64, 0xFE);
            } else if (cmd[0] == 's' && cmd[1] == 'h' && cmd[2] == 'u' && cmd[3] == 't' && cmd[4] == 'd' && cmd[5] == 'o' && cmd[6] == 'w' && cmd[7] == 'n' && cmd[8] == '\0') {
                /* 注：0x604 是 QEMU 的 ACPI PM1a 端口；实体机（尤其笔记本）通常
                 * 不监听该端口，需读 FADT 的 SLP_TYP/SLP_EN 才能真正关机。
                 * 当前保留 QEMU 兼容路径，实体机行为不做保证。 */
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
            caret = 0;
            prompt_col = cursor_x;   /* 记录新提示符后的起始列 */
            cursor_x = prompt_col;
        } else if (c == 0x03) {      /* 左箭头：光标左移 */
            if (caret > 0) caret--;
        } else if (c == 0x04) {      /* 右箭头：光标右移 */
            if (caret < pos) caret++;
        } else if (c == '\b') {      /* 退格：删除光标前一字符 */
            if (caret > 0) {
                for (int i = caret - 1; i < pos - 1; i++) cmd[i] = cmd[i + 1];
                pos--;
                caret--;
            }
        } else if (c >= ' ' && pos < 255) {  /* 可打印字符：在光标处插入 */
            for (int i = pos; i > caret; i--) cmd[i] = cmd[i - 1];
            cmd[caret] = c;
            pos++;
            caret++;
        }

        /* 统一重画当前输入行（支持插入/删除/方向键后的正确显示） */
        cursor_x = prompt_col + caret;
        int cols = (int)(bootboot.fb_width / 9);
        for (int xc = prompt_col; xc < cols; xc++) {
            for (int y = 0; y < 17; y++) {
                for (int x = 0; x < 9; x++) {
                    fb_putpixel(xc * 9 + x, cursor_y * 17 + y, 0x000000);
                }
            }
        }
        for (int k = 0; k < pos; k++) {
            fb_putchar(prompt_col + k, cursor_y, cmd[k], 0xFFFFFF);
        }
        for (int i = 0; i < 8; i++) {
            fb_putpixel(cursor_x * 9 + i, cursor_y * 17 + 15, 0xFFFFFF);
            fb_putpixel(cursor_x * 9 + i, cursor_y * 17 + 16, 0xFFFFFF);
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

/* 内核主流程：初始化堆 → 注册模块 → 开机动画 → 进入 Shell。
 * desktop 模块暂未接线（render 函数未实现），故此处注释掉注册。 */

/* [test] PIT 100Hz 的 tick 计数, 中断上线的活体证据:
 * 右上角三位数计数器每 32 tick 刷一次, 跳动 = IRQ0 活着 */
static volatile unsigned int pit_ticks = 0;
static void pit_tick(interrupt_frame_t* f) {
    (void)f;
    pit_ticks++;
    if ((pit_ticks & 31) == 0) {
        unsigned int t = pit_ticks;
        int cols = (int)(bootboot.fb_width / 9);
        for (int x = 0; x < 27; x++)
            for (int y = 0; y < 16; y++)
                fb_putpixel((cols - 3) * 9 + x, y, 0x000000);
        fb_putchar(cols - 3, 0, '0' + (t / 100) % 10, 0x00FF00);
        fb_putchar(cols - 2, 0, '0' + (t / 10) % 10, 0x00FF00);
        fb_putchar(cols - 1, 0, '0' + t % 10, 0x00FF00);
    }
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

    /* [test] 中断上线: IDT + PIC 重映射 + PIT 定时器
     * 开机动画期间 cli 着, 这里装好再 sti */
    idt_init();
    pic_remap();
    register_interrupt_handler(32, pit_tick);
    pic_clear_mask(0);          /* IRQ0 计时器 */
    outb(0x43, 0x36);           /* 通道0 方波 */
    outb(0x40, 11932 & 0xFF);   /* 1193182Hz / 100Hz */
    outb(0x40, 11932 >> 8);
    enable_interrupts();
    for (volatile long w = 0; w < 50000000; w++);
    fb_print("[test] interrupts ");
    if (pit_ticks > 10) fb_print("ALIVE, ticks=");
    else fb_print("DEAD, ticks=");
    print_int((int)pit_ticks);
    fb_print("\n");

    fb_print("MimicryOS Shell\n");
    fb_print("Type help for commands\n");

    shell();
}

/* 内核栈放在 .bss 段内（链接器映射的虚拟地址，保证可写），
 * 避免原用裸物理地址 0x9F000 作栈在长模式页表下未映射而崩溃 */
__attribute__((used)) static unsigned char kernel_stack[16384] __attribute__((aligned(16)));

__attribute__((used)) void _start(void) {
    __asm__ volatile("cli");
    __asm__ volatile("movq %0, %%rsp" : : "r"(kernel_stack + 16384) : "memory");
    kernel_main();
}
