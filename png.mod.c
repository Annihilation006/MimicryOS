#include "module.h"

void fb_putpixel(int x, int y, unsigned int color);
void fb_print(const char* str);
void* kmalloc(unsigned int size);
void kfree(void* ptr);

#define STBI_NO_STDLIB
#define STBI_NO_STDIO
#define STBI_ASSERT(x)
#define STB_IMAGE_IMPLEMENTATION
#define STBI_ONLY_PNG
#include "stb_image.h"

#define STBI_MALLOC(sz) kmalloc(sz)
#define STBI_REALLOC(p,newsz) kmalloc(newsz)
#define STBI_FREE(p) kfree(p)

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
void* malloc(unsigned long size) { return kmalloc((unsigned int)size); }
void free(void* ptr) { kfree(ptr); }
void* realloc(void* ptr, unsigned long size) {
    (void)ptr;
    return kmalloc((unsigned int)size);
}
