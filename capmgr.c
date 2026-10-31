/* capmgr.c — 能力槽生命周期管理 (C 管理层)
 * 策略簿记在这边 (C), 表原语在 Rust CSpace:
 *   rust_cap_release(token) -> 回收槽数 (级联 revoke)
 *   rust_cap_live()         -> 存活计数
 * NO_CAP = 无效标记; 释放计数只增不减, 供运行期自证回收确实发生。 */
#include "capmgr.h"

/* Rust 侧原语 (rust_kernel.rs #[no_mangle] 导出) */
extern unsigned int rust_cap_revoke(uint32_t slot);
extern unsigned int rust_cap_live(void);

#define NO_CAP 0xFFFFFFFFu

static unsigned int released_total; /* 累计回收次数 */

unsigned int capmgr_release(uint32_t token)
{
    unsigned int freed;

    if (token == NO_CAP || token == 0)
        return 0; /* 0 号与全 1 是保留值, 不可能是真令牌 */

    freed = rust_cap_revoke(token);
    if (freed > 0)
        released_total += freed;
    return freed;
}

unsigned int capmgr_live(void)
{
    return rust_cap_live();
}

unsigned int capmgr_released_total(void)
{
    return released_total;
}
