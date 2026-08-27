/* task.h */
#ifndef TASK_H
#define TASK_H

typedef enum {
    TASK_READY, TASK_RUNNING, TASK_BLOCKED, TASK_SLEEPING, TASK_TERMINATED
} task_state_t;

typedef enum {
    PRIORITY_LOW = 0, PRIORITY_NORMAL = 1, PRIORITY_HIGH = 2, PRIORITY_CRITICAL = 3
} task_priority_t;

typedef struct task {
    unsigned int esp;
    unsigned int ebp;
    unsigned int eip;
    unsigned int stack_top;
    unsigned int stack_bottom;
    int tid;
    task_state_t state;
    task_priority_t priority;
    char name[32];
    unsigned int sleep_ticks;
    struct task* next;
} task_t;

typedef void (*task_func_t)(void);

void scheduler_init(void);
task_t* task_create(const char* name, task_func_t func, task_priority_t priority);
void task_exit(void);
void task_sleep(unsigned int ticks);
void task_yield(void);
void scheduler_run(void);
void task_list(void);
task_t* get_current_task(void);

#endif