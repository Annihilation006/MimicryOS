#ifndef MEMORY_H
#define MEMORY_H

typedef unsigned int size_t;
#define NULL ((void*)0)

typedef struct memory_block {
    size_t size;
    int is_free;
    struct memory_block* next;
    struct memory_block* prev;
} memory_block_t;

void memory_init(void* start, size_t size);
void* kmalloc(size_t size);
void* kcalloc(size_t num, size_t size);
void* krealloc(void* ptr, size_t new_size);
void kfree(void* ptr);
void memory_info(void);
void memory_dump(void);
int memory_check(void);

#endif