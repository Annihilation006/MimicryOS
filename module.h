#ifndef MODULE_H
#define MODULE_H

#define NULL ((void*)0)
#define MODULE_MAGIC 0x4D4F4455
#define MODULE_EXPORT __attribute__((visibility("default")))

typedef struct module {
    unsigned int magic;
    const char* name;
    const char* desc;
    int (*init)(void);
    struct module* next;
} module_t;

void module_init(void);
int module_register(module_t* mod);
void module_list(void);

#endif
