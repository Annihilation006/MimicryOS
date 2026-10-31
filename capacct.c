/* capacct.c — 能力槽归属账本与退出回收
 * [test] Rust 侧 CSpace 负责发放与级联撤销 (seL4 风格派生树),
 * 本模块只记账: note 记归属, reap 收集某任务名下全部槽位并清账。
 * 任务退出路径调用 reap → 逐个 rust_cap_revoke, 槽位回到空闲栈。 */
#include "capacct.h"

#define CACCT_SLOTS 256   /* 与 rust_kernel.rs 的 NCAPS 一致 */
#define CACCT_NONE  0xFFu /* 无主 (内核自持的槽不记账) */

static unsigned char cap_owner[CACCT_SLOTS];

void capacct_init(void) {
    for (int i = 0; i < CACCT_SLOTS; i++) {
        cap_owner[i] = CACCT_NONE;
    }
}

void capacct_note(unsigned int slot, unsigned char task) {
    if (slot < CACCT_SLOTS) {
        cap_owner[slot] = task;
    }
}

unsigned char capacct_owner(unsigned int slot) {
    return (slot < CACCT_SLOTS) ? cap_owner[slot] : CACCT_NONE;
}

/* 收集 task 名下全部槽位 (同时清账), 返回数量; 调用方随后逐个 revoke */
int capacct_reap(unsigned char task, unsigned int *out, int max) {
    int n = 0;
    for (int i = 0; i < CACCT_SLOTS && n < max; i++) {
        if (cap_owner[i] == task) {
            out[n++] = (unsigned int)i;
            cap_owner[i] = CACCT_NONE;
        }
    }
    return n;
}
