/* task.c - 多任务调度器（修复栈布局） */
#include "task.h"
#include "console.h"
#include "idt.h"
#include "pic.h"
#include "memory.h"

#define MAX_TASKS 32
#define TASK_STACK_SIZE 4096

static task_t* task_list_head = NULL;
static task_t* current_task = NULL;
static int next_tid = 0;
static int task_count = 0;
static volatile unsigned int system_ticks = 0;

extern void switch_to_task(unsigned int esp);

static void task_wrapper(void) {
    task_t* task = current_task;
    __asm__ volatile("sti");
    if (task && task->eip) {
        task_func_t func = (task_func_t)task->eip;
        func();
    }
    task_exit();
}

static void timer_handler(interrupt_frame_t* frame) {
    (void)frame;
    system_ticks++;
    task_t* t = task_list_head;
    while (t) {
        if (t->state == TASK_SLEEPING) {
            t->sleep_ticks--;
            if (t->sleep_ticks <= 0) t->state = TASK_READY;
        }
        t = t->next;
    }
    pic_send_eoi(0);
    scheduler_run();
}

void scheduler_init(void) {
    task_list_head = NULL;
    current_task = NULL;
    next_tid = 0;
    task_count = 0;
    system_ticks = 0;

    outb(0x43, 0x36);
    outb(0x40, 100 & 0xFF);
    outb(0x40, (100 >> 8) & 0xFF);

    // 暂时不注册时钟中断，避免开启中断后崩溃
    // register_interrupt_handler(IRQ_BASE + 0, timer_handler);
    // pic_clear_mask(0);

    print("Scheduler initialized (100Hz).\n");
}

task_t* task_create(const char* name, task_func_t func, task_priority_t priority) {
    if (task_count >= MAX_TASKS) return NULL;

    task_t* task = (task_t*)kmalloc(sizeof(task_t));
    if (!task) return NULL;

    void* stack = kmalloc(TASK_STACK_SIZE);
    if (!stack) {
        kfree(task);
        return NULL;
    }

    task->tid = next_tid++;
    task->state = TASK_READY;
    task->priority = priority;
    task->sleep_ticks = 0;
    task->next = NULL;
    task->eip = (unsigned int)func;

    int i = 0;
    while (name[i] && i < 31) {
        task->name[i] = name[i];
        i++;
    }
    task->name[i] = '\0';

    task->stack_bottom = (unsigned int)stack;
    task->stack_top = (unsigned int)stack + TASK_STACK_SIZE;

    unsigned int* esp = (unsigned int*)task->stack_top;
    *--esp = 0x10;                    // SS
    *--esp = task->stack_top;         // ESP
    *--esp = 0x202;                   // EFLAGS
    *--esp = 0x08;                    // CS
    *--esp = (unsigned int)task_wrapper; // EIP
    *--esp = 0; // EDI
    *--esp = 0; // ESI
    *--esp = 0; // EBX
    *--esp = 0; // EBP

    task->esp = (unsigned int)esp;
    task->ebp = task->esp;

    if (!task_list_head || task->priority > task_list_head->priority) {
        task->next = task_list_head;
        task_list_head = task;
    } else {
        task_t* cur = task_list_head;
        while (cur->next && cur->next->priority >= task->priority) {
            cur = cur->next;
        }
        task->next = cur->next;
        cur->next = task;
    }

    task_count++;
    return task;
}

void task_exit(void) {
    if (current_task) {
        current_task->state = TASK_TERMINATED;
        task_count--;
        scheduler_run();
        while(1) {
            __asm__ volatile("hlt");
        }
    }
}

void task_sleep(unsigned int ticks) {
    if (current_task && ticks > 0) {
        current_task->state = TASK_SLEEPING;
        current_task->sleep_ticks = ticks;
        scheduler_run();
    }
}

void task_yield(void) {
    scheduler_run();
}

void scheduler_run(void) {
    if (!task_list_head) return;

    task_t* next = NULL;
    task_t* candidate = task_list_head;

    for (int i = 0; i < task_count; i++) {
        if (candidate && candidate->state == TASK_READY) {
            if (!next || candidate->priority > next->priority) {
                next = candidate;
                if (candidate->priority == PRIORITY_CRITICAL) break;
            }
        }
        candidate = candidate->next;
        if (!candidate) candidate = task_list_head;
    }

    if (!next) {
        __asm__ volatile("sti");
        __asm__ volatile("hlt");
        return;
    }

    if (current_task != next) {
        task_t* prev = current_task;
        if (prev && prev->state == TASK_RUNNING) {
            prev->state = TASK_READY;
        }
        current_task = next;
        current_task->state = TASK_RUNNING;

        if (prev) {
            unsigned int saved_esp;
            __asm__ volatile("mov %%esp, %0" : "=r"(saved_esp));
            prev->esp = saved_esp;
        }

        switch_to_task(current_task->esp);
    }
}

task_t* get_current_task(void) {
    return current_task;
}

void task_list(void) {
    task_t* t = task_list_head;
    print("Task List:\n");
    print("  TID | Name          | State     | Priority\n");
    print("  ----+---------------+-----------+----------\n");
    while (t) {
        print("  ");
        print_int(t->tid);
        print("   | ");
        print(t->name);
        int len = 0;
        while (t->name[len]) len++;
        for (int i = len; i < 15; i++) print(" ");
        print("| ");
        switch (t->state) {
            case TASK_READY: print("READY     "); break;
            case TASK_RUNNING: print("RUNNING   "); break;
            case TASK_BLOCKED: print("BLOCKED   "); break;
            case TASK_SLEEPING: print("SLEEPING  "); break;
            case TASK_TERMINATED: print("TERMINATED"); break;
        }
        print("| ");
        switch (t->priority) {
            case PRIORITY_LOW: print("LOW"); break;
            case PRIORITY_NORMAL: print("NORMAL"); break;
            case PRIORITY_HIGH: print("HIGH"); break;
            case PRIORITY_CRITICAL: print("CRITICAL"); break;
        }
        print("\n");
        t = t->next;
    }
}