/* ---- png 模块：PNG 解码渲染 ----
 * 基于 stb_image.h（仅启用 PNG）解码，内存映射到内核堆 kmalloc/kfree。
 * 注意：STBI_* 内存宏与 STBI_NO_* 开关必须在 include stb_image.h 之前定义，
 * 否则 stb 会回退到默认 malloc（freestanding 下链接崩溃）。 */
#include "module.h"

void fb_putpixel(int x, int y, unsigned int color);
void fb_print(const char* str);
void* kmalloc(unsigned int size);
void kfree(void* ptr);
unsigned int kmalloc_size(void* ptr);
/* 供 stb_image 使用的内存操作（freestanding 无 string.h，需在 include 前声明） */
void* memset(void* dest, int val, unsigned long count);
void* memcpy(void* dest, const void* src, unsigned long count);

/* 正确语义的 realloc：拷贝 min(旧,新) 字节并释放旧块。
 * 原实现直接返回新块、不拷贝不释放，导致 PNG 解码内存泄漏与数据错乱。 */
static void* stbi_realloc_impl(void* p, unsigned long newsz) {
    if (!p) return kmalloc((unsigned int)newsz);
    unsigned int old = kmalloc_size(p);
    void* np = kmalloc((unsigned int)newsz);
    if (!np) return NULL;
    unsigned int n = old < newsz ? old : (unsigned int)newsz;
    for (unsigned int i = 0; i < n; i++) ((char*)np)[i] = ((char*)p)[i];
    kfree(p);
    return np;
}

/* 注意：这些宏必须在 include stb_image.h 之前定义，否则 stb 回退到
 * 默认 malloc/free，且原代码里宏写在 include 之后是无效的。 */
#define STBI_NO_STDLIB
#define STBI_NO_STDIO
#define STBI_NO_LINEAR
#define STBI_NO_HDR
#define STBI_NO_THREAD_LOCALS
#define STBI_ASSERT(x)
#define STBI_ONLY_PNG
#define STBI_MALLOC(sz) kmalloc(sz)
#define STBI_REALLOC(p,newsz) stbi_realloc_impl(p,newsz)
#define STBI_FREE(p) kfree(p)
#define STB_IMAGE_IMPLEMENTATION
#include "stb_image.h"

void* memset(void* dest, int val, unsigned long count) {
    char* d = dest;
    for (unsigned long i = 0; i < count; i++) d[i] = val;
    return dest;
}
void* memcpy(void* dest, const void* src, unsigned long count) {
    char* d = dest;
    const char* s = src;
    for (unsigned long i = 0; i < count; i++) d[i] = s[i];
    return dest;
}
double pow(double x, double y) { (void)x; (void)y; return 0; }

static unsigned char* png_data = NULL;
static int png_width = 0;
static int png_height = 0;

void png_load(void* data, int size) {
    int channels;
    unsigned char* decoded = stbi_load_from_memory(data, size, &png_width, &png_height, &channels, 4);
    if (decoded) {
        png_data = decoded;
    }
}

void png_render(int x, int y) {
    if (!png_data) return;
    for (int i = 0; i < png_height; i++) {
        for (int j = 0; j < png_width; j++) {
            int idx = (i * png_width + j) * 4;
            unsigned int r = png_data[idx];
            unsigned int g = png_data[idx + 1];
            unsigned int b = png_data[idx + 2];
            unsigned int color = (r << 16) | (g << 8) | b;
            fb_putpixel(x + j, y + i, color);
        }
    }
}

static int png_init(void) {

    return 0;
}

MODULE_EXPORT module_t png_module = {
    .magic = MODULE_MAGIC,
    .name = "png",
    .desc = "PNG renderer",
    .init = png_init,
    .next = NULL
};
