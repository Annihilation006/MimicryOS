/* module_manager.c */
#include "module.h"
#include "console.h"
#include "string.h"

static module_t* module_list_head = NULL;

void module_init(void) {
    module_list_head = NULL;
    print("Module system initialized.\n");
}

int module_register(module_t* mod) {
    if (mod->magic != MODULE_MAGIC) return -1;
    if (module_find(mod->name)) return -1;
    
    mod->next = module_list_head;
    module_list_head = mod;
    
    if (mod->init_func) {
        if (mod->init_func() == 0) return 0;
    }
    return 0;
}

int module_unregister(const char* name) {
    module_t* cur = module_list_head;
    module_t* prev = NULL;
    while (cur) {
        if (strcmp(cur->name, name) == 0) {
            if (cur->exit_func) cur->exit_func();
            if (prev) prev->next = cur->next;
            else module_list_head = cur->next;
            return 0;
        }
        prev = cur;
        cur = cur->next;
    }
    return -1;
}

void module_list(void) {
    module_t* cur = module_list_head;
    int count = 0;
    print("Modules:\n");
    while (cur) {
        print("  ["); print_int(count++); print("] ");
        print(cur->name); print(" - "); print(cur->description); print("\n");
        cur = cur->next;
    }
    if (count == 0) print("  None.\n");
}

module_t* module_find(const char* name) {
    module_t* cur = module_list_head;
    while (cur) {
        if (strcmp(cur->name, name) == 0) return cur;
        cur = cur->next;
    }
    return NULL;
}