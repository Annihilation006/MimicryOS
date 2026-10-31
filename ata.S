/* ata.S — ATA PIO 磁盘驱动, 纯汇编 (GAS / AT&T 语法)
 * [test] 重量级组件: 设备识别 (IDENTIFY + LBA48 探测)、
 * LBA48 多扇区读写、超时保护、写后 cache flush。
 *
 * 主通道寄存器 (x86 标准):
 *   0x1F0 数据   0x1F2 扇区数   0x1F3-6 LBA/设备
 *   0x1F7 命令/状态   0x3F6 设备控制
 *
 * 状态位: BSY=0x80 DRQ=0x08 ERR=0x01 DF=0x20
 * 命令:   0x20 读LBA28  0x24 读LBA48  0x30 写LBA28
 *         0x34 写LBA48  0xE7 flush    0xEC identify
 *
 * 导出接口 (C ABI):
 *   void ata_drv_init(void)                        识别设备, 置 ata_drv_ok/lba48
 *   int  ata_drv_read(u64 lba, u16 count, void*buf)  返回 0=成功 -1=失败
 *   int  ata_drv_write(u64 lba, u16 count, void*buf) 同上
 *   int  ata_drv_ok(void)                          设备就绪?
 *   int  ata_drv_lba48(void)                       LBA48 支持?
 * 静态状态:
 *   ata_drv_present / ata_drv_lba48 (byte, 0/1)
 *
 * 注意: 内核链在高地址 (--no-pie, -mcmodel=large)。AT&T 汇编里
 * 访问静态数据用 movabsq $sym, %rax 取 64 位地址再间接读写。
 */

.section .data
.globl ata_drv_present
.globl ata_drv_lba48
ata_drv_present: .byte 0
ata_drv_lba48:   .byte 0

.section .text
.code64

.globl ata_drv_init
.globl ata_drv_read
.globl ata_drv_write
.globl ata_drv_ok
.globl ata_drv_has48

/* ---- 内部: r8d = 端口, 等待 BSY 清零; r9 = 超时计数 ----
 * 返回 ZF=1 表示就绪 (用 test 语义: al 返回最终状态) */
ata_wait_bsy:
    xor %r9d, %r9d
1:  movw $0x1F7, %dx
    inb %dx, %al
    testb $0x80, %al            /* BSY */
    jz 2f
    inc %r9d
    cmpq $5000000, %r9
    jb 1b
2:  ret

/* ---- 内部: 等待 DRQ 置位 (数据就绪), 超时返回 -1 于 eax ---- */
ata_wait_drq:
    xor %r9d, %r9d
1:  movw $0x1F7, %dx
    inb %dx, %al
    testb $0x08, %al            /* DRQ */
    jnz 2f
    testb $0x01, %al            /* ERR 提前退出 */
    jnz 3f
    inc %r9d
    cmpq $5000000, %r9
    jb 1b
3:  movl $-1, %eax
    ret
2:  xor %eax, %eax
    ret

/* ---- ata_drv_init: IDENTIFY DEVICE, 探测 LBA48 ---- */
.globl ata_drv_init
ata_drv_init:
    pushq %rbx
    /* 选主盘: 0x1F6 = 0xA0 */
    movw $0x1F6, %dx
    movb $0xA0, %al
    outb %al, %dx
    call ata_wait_bsy
    /* IDENTIFY: 0x1F7 = 0xEC */
    movw $0x1F7, %dx
    movb $0xEC, %al
    outb %al, %dx
    call ata_wait_drq
    testl %eax, %eax
    jz .init_fail
    /* 读 256 字识别字到栈上临时区 (512B), 探 word83 bit10 = LBA48 */
    subq $512, %rsp
    movq %rsp, %rdi
    cld
    movw $0x1F0, %dx
    movl $256, %ecx
    rep insw
    /* word83 在偏移 166 (0xA6) */
    movw 166(%rsp), %ax
    testw $0x0400, %ax          /* bit10 = LBA48 */
    setnz %bl
    /* 扇区总数 word60/61 (LBA28 可寻址数), 非零即有盘 */
    movl 120(%rsp), %ebx        /* word60/61 偏移 120 */
    addq $512, %rsp
    testl %ebx, %ebx
    jz .init_fail
    /* 置状态静态: movabsq 取高地址符号 */
    movabsq $ata_drv_present, %rax
    movb $1, (%rax)
    testb %bl, %bl
    jz .no48
    movabsq $ata_drv_lba48, %rax
    movb $1, (%rax)
.no48:
    popq %rbx
    xor %eax, %eax
    ret
.init_fail:
    movabsq $ata_drv_present, %rax
    movb $0, (%rax)
    popq %rbx
    movl $-1, %eax
    ret

/* ---- ata_drv_ok: 返回设备就绪标志 ---- */
.globl ata_drv_ok
.globl ata_drv_has48
ata_drv_ok:
    movabsq $ata_drv_present, %rax
    movzbl (%rax), %eax
    ret

/* ---- ata_drv_has48: 返回 LBA48 支持标志 ---- */
.globl ata_drv_has48
ata_drv_has48:
    movabsq $ata_drv_lba48, %rax
    movzbl (%rax), %eax
    ret

/* ---- ata_drv_read(rdi=lba u64, rsi=count, rdx=buf) -> eax: 0/-1 ----
 * LBA48 读: 两次写扇区数/LBA (高低字节), 命令 0x24, rep insw */
.globl ata_drv_read
ata_drv_read:
    pushq %rbx
    pushq %r12
    pushq %r13
    pushq %r14
    movq %rdx, %r12             /* buf */
    movq %rsi, %r13             /* count */
    movq %rdi, %r14             /* lba */
    /* 等 BSY */
    call ata_wait_bsy
    /* 设备寄存器: 0x40 | (master) */
    movw $0x1F6, %dx
    movb $0x40, %al
    outb %al, %dx
    /* 扇区数高字节 */
    movw $0x1F2, %dx
    movl %r13d, %eax
    shrl $8, %eax
    outb %al, %dx
    /* LBA 高 3 字节 (bits 24-47) */
    movw $0x1F3, %dx
    movl %r14d, %eax
    shrl $24, %eax
    outb %al, %dx
    movw $0x1F4, %dx
    movl %r14d, %eax
    shrl $32, %eax
    outb %al, %dx
    movw $0x1F5, %dx
    movl %r14d, %eax
    shrl $40, %eax
    outb %al, %dx
    /* 扇区数低字节 */
    movw $0x1F2, %dx
    movl %r13d, %eax
    outb %al, %dx
    /* LBA 低 3 字节 (bits 0-23) */
    movw $0x1F3, %dx
    movl %r14d, %eax
    outb %al, %dx
    movw $0x1F4, %dx
    movl %r14d, %eax
    shrl $8, %eax
    outb %al, %dx
    movw $0x1F5, %dx
    movl %r14d, %eax
    shrl $16, %eax
    outb %al, %dx
    /* 命令 0x24 = 读 LBA48 */
    movw $0x1F7, %dx
    movb $0x24, %al
    outb %al, %dx
    /* 等数据就绪 */
    call ata_wait_drq
    testl %eax, %eax
    js .rd_fail
    /* rep insw: count*256 字 */
    movw $0x1F0, %dx
    movq %r12, %rdi
    cld
    movl %r13d, %ecx
    shll $8, %ecx               /* count * 256 字 */
    rep insw
    movl $0, %eax
    jmp .rd_out
.rd_fail:
    movl $-1, %eax
.rd_out:
    popq %r14
    popq %r13
    popq %r12
    popq %rbx
    ret

/* ---- ata_drv_write(rdi=lba, rsi=count, rdx=buf) -> eax: 0/-1 ----
 * LBA48 写: 数据 rep outsw 后 cache flush (0xE7) */
.globl ata_drv_write
ata_drv_write:
    pushq %rbx
    pushq %r12
    pushq %r13
    pushq %r14
    movq %rdx, %r12             /* buf */
    movq %rsi, %r13             /* count */
    movq %rdi, %r14             /* lba */
    call ata_wait_bsy
    movw $0x1F6, %dx
    movb $0x40, %al
    outb %al, %dx
    movw $0x1F2, %dx
    movl %r13d, %eax
    shrl $8, %eax
    outb %al, %dx
    movw $0x1F3, %dx
    movl %r14d, %eax
    shrl $24, %eax
    outb %al, %dx
    movw $0x1F4, %dx
    movl %r14d, %eax
    shrl $32, %eax
    outb %al, %dx
    movw $0x1F5, %dx
    movl %r14d, %eax
    shrl $40, %eax
    outb %al, %dx
    movw $0x1F2, %dx
    movl %r13d, %eax
    outb %al, %dx
    movw $0x1F3, %dx
    movl %r14d, %eax
    outb %al, %dx
    movw $0x1F4, %dx
    movl %r14d, %eax
    shrl $8, %eax
    outb %al, %dx
    movw $0x1F5, %dx
    movl %r14d, %eax
    shrl $16, %eax
    outb %al, %dx
    /* 命令 0x34 = 写 LBA48 */
    movw $0x1F7, %dx
    movb $0x34, %al
    outb %al, %dx
    call ata_wait_drq
    testl %eax, %eax
    js .wr_fail
    /* rep outsw: count*256 字 */
    movw $0x1F0, %dx
    movq %r12, %rsi
    cld
    movl %r13d, %ecx
    shll $8, %ecx
    rep outsw
    /* cache flush (0xE7) + 等 BSY 清 */
    movw $0x1F7, %dx
    movb $0xE7, %al
    outb %al, %dx
    call ata_wait_bsy
    movl $0, %eax
    jmp .wr_out
.wr_fail:
    movl $-1, %eax
.wr_out:
    popq %r14
    popq %r13
    popq %r12
    popq %rbx
    ret
