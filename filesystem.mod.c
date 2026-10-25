#include "module.h"

void fb_print(const char* str);
void* kmalloc(unsigned int size);
void kfree(void* ptr);
int strcmp(const char* s1, const char* s2);

typedef struct mem_file {
    char name[32];
    char content[256];
    struct mem_file* next;
} mem_file_t;

static mem_file_t* file_list = NULL;

void file_create(const char* name) {
    mem_file_t* f = kmalloc(sizeof(mem_file_t));
    int i = 0;
    while (name[i] && i < 31) {
        f->name[i] = name[i];
        i++;
    }
    f->name[i] = '\0';
    f->content[0] = '\0';
    f->next = file_list;
    file_list = f;
    fb_print("File created\n");
}

void file_write(const char* name, const char* content) {
    mem_file_t* f = file_list;
    while (f) {
        if (strcmp(f->name, name) == 0) {
            int i = 0;
            while (content[i] && i < 255) {
                f->content[i] = content[i];
                i++;
            }
            f->content[i] = '\0';
            fb_print("Written\n");
            return;
        }
        f = f->next;
    }
    fb_print("File not found\n");
}

void file_cat(const char* name) {
    mem_file_t* f = file_list;
    while (f) {
        if (strcmp(f->name, name) == 0) {
            fb_print(f->content);
            fb_print("\n");
            return;
        }
        f = f->next;
    }
    fb_print("File not found\n");
}




void file_size(const char* name) {
    mem_file_t* f = file_list;
    while (f) {
        if (strcmp(f->name, name) == 0) {
            int len = 0;
            while (f->content[len]) len++;
            fb_print("Size: ");
            char buf[16];
            int n = len, i = 0;
            if (n == 0) buf[i++] = '0';
            while (n > 0) {
                buf[i++] = '0' + (n % 10);
                n /= 10;
            }
            while (i > 0) {
                char s[2];
                s[0] = buf[--i];
                s[1] = '\0';
                fb_print(s);
            }
            fb_print(" bytes\n");
            return;
        }
        f = f->next;
    }
    fb_print("File not found\n");
}
void file_delete(const char* name) {
    mem_file_t* f = file_list;
    mem_file_t* prev = NULL;
    while (f) {
        if (strcmp(f->name, name) == 0) {
            if (prev) prev->next = f->next;
            else file_list = f->next;
            kfree(f);
            fb_print("Deleted\n");
            return;
        }
        prev = f;
        f = f->next;
    }
    fb_print("File not found\n");
}
void file_list_all(void) {
    mem_file_t* f = file_list;
    while (f) {
        fb_print(f->name);
        fb_print("\n");
        f = f->next;
    }
}
static int filesystem_init(void) {

    return 0;
}

MODULE_EXPORT module_t filesystem_module = {
    .magic = MODULE_MAGIC,
    .name = "filesystem",
    .desc = "Memory filesystem",
    .init = filesystem_init,
    .next = NULL
};
