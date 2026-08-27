/* calculator.mod.c */
#include "module.h"

int calc_add(int a, int b) { return a + b; }
int calc_sub(int a, int b) { return a - b; }
int calc_mul(int a, int b) { return a * b; }
int calc_div(int a, int b) { return b ? a / b : 0; }

static int calc_init(void) { return 0; }

MODULE_EXPORT module_t calculator_module = {
    .magic = MODULE_MAGIC,
    .version = MODULE_VERSION,
    .name = "calculator",
    .description = "Basic arithmetic",
    .type = MODULE_APP,
    .init_func = calc_init,
    .exit_func = NULL,
    .next = NULL
};