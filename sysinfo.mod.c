/* sysinfo.mod.c */
#include "module.h"
#include "console.h"

void show_system_info(void) {
    print("===============================================\n");
    print("  MyOS v0.4 - Multitasking Kernel\n");
    print("  Architecture: x86 (32-bit)\n");
    print("  Features: IDT, PIC, PIT, Multitasking\n");
    print("===============================================\n");
}

static int sysinfo_init(void) { return 0; }

MODULE_EXPORT module_t sysinfo_module = {
    .magic = MODULE_MAGIC,
    .version = MODULE_VERSION,
    .name = "sysinfo",
    .description = "System information",
    .type = MODULE_SERVICE,
    .init_func = sysinfo_init,
    .exit_func = NULL,
    .next = NULL
};