/* tasks.mod.c */
#include "module.h"
#include "task.h"
#include "console.h"

void counter_task(void) {
    int count = 0;
    while (1) {
        print("[Counter] ");
        print_int(count++);
        print("\n");
        task_sleep(100);
    }
}

void prime_task(void) {
    int num = 2;
    while (1) {
        int is_prime = 1;
        for (int i = 2; i * i <= num; i++) {
            if (num % i == 0) { is_prime = 0; break; }
            task_yield();
        }
        if (is_prime) {
            print("[Prime] ");
            print_int(num);
            print("\n");
            task_sleep(200);
        }
        num++;
    }
}

static int tasks_init(void) {
    task_create("counter", counter_task, PRIORITY_NORMAL);
    task_create("prime", prime_task, PRIORITY_LOW);
    return 0;
}

MODULE_EXPORT module_t tasks_module = {
    .magic = MODULE_MAGIC,
    .version = MODULE_VERSION,
    .name = "tasks",
    .description = "Example tasks",
    .type = MODULE_APP,
    .init_func = tasks_init,
    .exit_func = NULL,
    .next = NULL
};