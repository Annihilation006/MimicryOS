/* capacct.h — 能力槽归属账本
 * [test] 记录 slot→持有任务, 任务退出时由调度路径整批收集,
 * 交给 Rust 侧 rust_cap_revoke 做级联回收, 槽位遂闭环。 */
#ifndef CAPACCT_H
#define CAPACCT_H

void capacct_init(void);
void capacct_note(unsigned int slot, unsigned char task);
unsigned char capacct_owner(unsigned int slot);
int capacct_reap(unsigned char task, unsigned int *out, int max);

#endif
