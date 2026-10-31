/* capmgr.h — 能力槽生命周期管理 (C 管理层)
 * 职责: 校验+登记+释放的簿记策略; 表原语在 Rust CSpace (free_stack 回收)。
 * rust_kernel 的 SYS_CLOSE(16) 调 capmgr_release; 释放走 rust_cap_release
 * (级联 revoke: 子能力一并回收)。 */
#ifndef CAPMGR_H
#define CAPMGR_H

#include <stdint.h>

/* 释放一个能力 (级联其派生子能力)。返回回收的槽位数 (0=令牌无效)。 */
unsigned int capmgr_release(uint32_t token);

/* 当前存活能力数 (来自 Rust CSpace live 计数) */
unsigned int capmgr_live(void);

/* 累计释放次数 (自证计数器, gdb 可读) */
unsigned int capmgr_released_total(void);

#endif
