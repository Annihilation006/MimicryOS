/* memory_test.mod.c */
#include "module.h"
#include "memory.h"
#include "console.h"

static int memtest_init(void) {
    print("Memory test:\n");
    void* p1 = kmalloc(100);
    void* p2 = kmalloc(200);
    void* p3 = kmalloc(300);
    print("  Allocated: "); print_hex((unsigned int)p1); print(", ");
    print_hex((unsigned int)p2); print(", "); print_hex((unsigned int)p3); print("\n");
    kfree(p2);
    void* p4 = kmalloc(150);
    print("  Reused: "); print_hex((unsigned int)p4); print("\n");
    kfree(p1); kfree(p3); kfree(p4);
    memory_check();
    return 0;
}

MODULE_EXPORT module_t memory_test_module = {
    .magic = MODULE_MAGIC,
    .version = MODULE_VERSION,
    .name = "memtest",
    .description = "Memory tests",
    .type = MODULE_APP,
    .init_func = memtest_init,
    .exit_func = NULL,
    .next = NULL
};