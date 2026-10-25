/* memory.c - 内存管理器 */
#include "memory.h"
#include "console.h"

#define ALIGN8(x) (((x) + 7) & ~7)
#define MIN_BLOCK_SIZE (sizeof(memory_block_t) + 8)

static memory_block_t* heap_start = NULL;
static size_t heap_total = 0;
static size_t heap_used = 0;
static int alloc_count = 0;
static int free_count = 0;

void memory_init(void* start, size_t size) {
    void* aligned = (void*)ALIGN8((unsigned int)start);
    size_t aligned_size = size - ((unsigned int)aligned - (unsigned int)start);
    aligned_size &= ~7;
    
    heap_start = (memory_block_t*)aligned;
    heap_total = aligned_size;
    heap_used = sizeof(memory_block_t);
    
    heap_start->size = aligned_size;
    heap_start->is_free = 1;
    heap_start->next = NULL;
    heap_start->prev = NULL;
    
    print("Memory manager initialized.\n");
    print("  Heap: ");
    print_hex((unsigned int)aligned);
    print(" - ");
    print_hex((unsigned int)aligned + aligned_size);
    print(" (");
    print_int((int)(aligned_size / 1024));
    print(" KB)\n");
}

static void split_block(memory_block_t* block, size_t size) {
    memory_block_t* new_block = (memory_block_t*)((char*)block + size);
    new_block->size = block->size - size;
    new_block->is_free = 1;
    new_block->next = block->next;
    new_block->prev = block;
    if (block->next) block->next->prev = new_block;
    block->next = new_block;
    block->size = size;
}

static void merge_blocks(memory_block_t* block) {
    while (block->next && block->next->is_free) {
        block->size += block->next->size;
        block->next = block->next->next;
        if (block->next) block->next->prev = block;
    }
    while (block->prev && block->prev->is_free) {
        block->prev->size += block->size;
        block->prev->next = block->next;
        if (block->next) block->next->prev = block->prev;
        block = block->prev;
    }
}

void* kmalloc(size_t size) {
    if (size == 0) return NULL;
    
    size_t total = ALIGN8(size) + sizeof(memory_block_t);
    if (total < MIN_BLOCK_SIZE) total = MIN_BLOCK_SIZE;
    
    memory_block_t* current = heap_start;
    while (current) {
        if (current->is_free && current->size >= total) {
            if (current->size > total + MIN_BLOCK_SIZE) {
                split_block(current, total);
            }
            current->is_free = 0;
            heap_used += current->size;
            alloc_count++;
            return (void*)((char*)current + sizeof(memory_block_t));
        }
        current = current->next;
    }
    
    print("kmalloc: Out of memory!\n");
    return NULL;
}

void* kcalloc(size_t num, size_t size) {
    size_t total = num * size;
    void* ptr = kmalloc(total);
    if (ptr) {
        char* p = (char*)ptr;
        for (size_t i = 0; i < total; i++) p[i] = 0;
    }
    return ptr;
}

void kfree(void* ptr) {
    if (!ptr) return;
    
    memory_block_t* block = (memory_block_t*)((char*)ptr - sizeof(memory_block_t));
    if (block->is_free) {
        print("kfree: Double free!\n");
        return;
    }
    
    block->is_free = 1;
    heap_used -= block->size;
    free_count++;
    merge_blocks(block);
}

void memory_info(void) {
    print("Memory Info:\n");
    print("  Total: ");
    print_int((int)heap_total);
    print(" bytes\n  Used:  ");
    print_int((int)heap_used);
    print(" bytes\n  Free:  ");
    print_int((int)(heap_total - heap_used));
    print(" bytes\n  Allocs: ");
    print_int(alloc_count);
    print("\n  Frees:  ");
    print_int(free_count);
    print("\n");
}

int memory_check(void) {
    memory_block_t* current = heap_start;
    int count = 0;
    size_t total = 0;
    
    while (current) {
        count++;
        total += current->size;
        if (current->next && current->next->prev != current) {
            print("Memory check FAILED!\n");
            return -1;
        }
        current = current->next;
    }
    
    print("Memory check passed: ");
    print_int(count);
    print(" blocks\n");
    return 0;
}

void memory_dump(void) {
    memory_block_t* current = heap_start;
    int i = 0;
    
    print("Memory Dump:\n");
    while (current) {
        print("  [");
        print_int(i++);
        print("] ");
        print_hex((unsigned int)current);
        print(" size=");
        print_int((int)current->size);
        print(" ");
        print(current->is_free ? "FREE" : "USED");
        print("\n");
        current = current->next;
    }
}