; ============================================================
; kbdrv.asm — IRQ1 中断驱动键盘驱动, 纯汇编
; 中断里读扫描码→翻译→压环形缓冲→EOI; 消费方: kbdrv_take (SYS_READ)
; 与 kbdrv_getc (shell 阻塞取)。翻译语义与 console.c keyboard_read 对齐:
; shift/caps 异或定大小写, e0 前缀方向键映 0x01-0x04, break 忽略
; 注意: 内核链在高地址 (--no-pie), 所有数据访问必须 [rel ...]
; ============================================================

[bits 64]

section .data
; set1 make → ASCII, 索引 = 扫描码, 共 60 项 (与 C keymap 逐项一致)
keymap:  db 0,27,'1','2','3','4','5','6','7','8','9','0','-','=',8
         db 9,'q','w','e','r','t','y','u','i','o','p','[',']',13
         db 0,'a','s','d','f','g','h','j','k','l',';',39,96
         db 0,92,'z','x','c','v','b','n','m',',','.','/',0
         db '*',0,' ',0
keymap_shift: db 0,27,'!','@','#','$','%','^','&','*','(',')','_','+',8
         db 9,'Q','W','E','R','T','Y','U','I','O','P','{','}',13
         db 0,'A','S','D','F','G','H','J','K','L',':',34,126
         db 0,'|','Z','X','C','V','B','N','M','<','>','?',0
         db '*',0,' ',0
KM_LEN equ keymap_shift - keymap

section .bss
align 16
global kb_buf
kb_buf:   resb 64          ; 队列 A: shell (kbdrv_getc)
kb_head:  resd 1
kb_tail:  resd 1
kb_buf2:  resb 64          ; 队列 B: 用户态 (kbdrv_take) — 每键双投, 消费者互不抢
kb_head2: resd 1
kb_tail2: resd 1
kb_shift: resd 1           ; shift 按下态
kb_caps:  resd 1           ; capslock 态
kb_e0:    resd 1           ; 扩展前缀态
global kb_cfg_read
global kb_cfg_write
kb_cfg_read:  resd 1       ; 自证: init 读到的 8042 配置字节
kb_cfg_write: resd 1       ; 自证: init 写回的 8042 配置字节

section .text

global kbdrv_init
global kbdrv_getc
global kbdrv_take
extern idt_set_gate

; ---------------- 初始化: 换 33 号门 + 放行 IRQ1 + 8042 中断使能 ----------------
kbdrv_init:
    mov edi, 33
    mov rsi, strict qword kbdrv_irq1
    mov edx, 0x08
    mov ecx, 0x8E
    call idt_set_gate
    in al, 0x21             ; 主 PIC 掩码, 清 bit1 放行 IRQ1
    and al, 0xFD
    out 0x21, al

    ; 8042 控制器配置字节: 置 bit0 (第一口中断使能 = IRQ1)。
    ; 前置排干: 输出缓冲里可能压着 BIOS/BOOTBOOT 时代的键盘应答 (0xFA),
    ; 不排干会把 ACK 当配置读, 真配置永远没被改
.drain:
    in al, 0x64
    test al, 1
    jz .drained
    in al, 0x60
    jmp .drain
.drained:
    mov al, 0x20           ; 读配置字节命令
    out 0x64, al
.waitr:
    in al, 0x64
    test al, 1
    jz .waitr              ; 等输出缓冲满
    in al, 0x60            ; al = 配置字节
    movzx eax, al
    mov [rel kb_cfg_read], eax
    or al, 0x01            ; bit0 = 键盘中断使能
    mov bl, al
    movzx eax, al
    mov [rel kb_cfg_write], eax
.ibf1:
    in al, 0x64
    test al, 2
    jnz .ibf1              ; 等输入缓冲空
    mov al, 0x60           ; 写配置字节命令
    out 0x64, al
.ibf2:
    in al, 0x64
    test al, 2
    jnz .ibf2
    mov al, bl
    out 0x60, al           ; 新配置: 键盘中断开
    ; 回读验证: 真配置应当已是 0xFB
    mov al, 0x20
    out 0x64, al
.waitr2:
    in al, 0x64
    test al, 1
    jz .waitr2
    in al, 0x60
    movzx eax, al
    mov [rel kb_cfg_read], eax

    xor eax, eax
    mov [rel kb_head], eax
    mov [rel kb_tail], eax
    mov [rel kb_head2], eax
    mov [rel kb_tail2], eax
    mov [rel kb_shift], eax
    mov [rel kb_caps], eax
    mov [rel kb_e0], eax
    ret

; ---------------- IRQ1 服务: 读码→翻译→入队→EOI ----------------
kbdrv_irq1:
    push rax
    push rcx
    push rdx

    xor eax, eax
    in al, 0x60             ; al = 扫描码
    mov edx, eax            ; edx = sc

    cmp edx, 0xE0           ; e0/e1 前缀: 置态忽略本字节
    je .set_e0
    cmp edx, 0xE1
    je .set_e0
    jmp .noe0
.set_e0:
    mov dword [rel kb_e0], 1
    jmp .done
.noe0:
    cmp edx, 0x55           ; 未知键
    je .done
    cmp edx, 0x5B           ; win make
    je .done
    cmp edx, 0x5C
    je .done
    cmp edx, 0xDB           ; win break
    je .done
    cmp edx, 0xDC
    je .done

    cmp edx, 0x2A           ; shift make/break
    je .shift_on
    cmp edx, 0x36
    je .shift_on
    cmp edx, 0xAA
    je .shift_off
    cmp edx, 0xB6
    je .shift_off

    cmp edx, 0x3A           ; caps make 翻转
    je .caps_flip
    cmp edx, 0xBA           ; caps break 忽略
    je .done

    cmp dword [rel kb_e0], 0 ; 扩展码: 方向键映 0x01-0x04
    je .norm
    mov dword [rel kb_e0], 0
    cmp edx, 0x48
    je .e0_up
    cmp edx, 0x47
    je .e0_up
    cmp edx, 0x50
    je .e0_dn
    cmp edx, 0x53
    je .e0_dn
    cmp edx, 0x4B
    je .e0_lf
    cmp edx, 0x4D
    je .e0_rt
    jmp .done
.e0_up:
    mov edx, 1
    jmp .push
.e0_dn:
    mov edx, 2
    jmp .push
.e0_lf:
    mov edx, 3
    jmp .push
.e0_rt:
    mov edx, 4
    jmp .push

.norm:
    cmp edx, 0x80           ; break 码忽略
    jae .done
    cmp edx, KM_LEN
    jae .done
    mov eax, [rel kb_shift] ; shift/caps 异或选表
    xor eax, [rel kb_caps]
    test eax, eax
    jz .plain
    movzx eax, byte [rel keymap_shift + rdx]
    jmp .gotch
.plain:
    movzx eax, byte [rel keymap + rdx]
.gotch:
    test eax, eax
    jz .done                ; 0 = 无字符
    mov edx, eax
    jmp .push

.shift_on:
    mov dword [rel kb_shift], 1
    jmp .done
.shift_off:
    mov dword [rel kb_shift], 0
    jmp .done
.caps_flip:
    mov eax, [rel kb_caps]
    xor eax, 1
    mov [rel kb_caps], eax
    jmp .done

.push:
    ; 双队列扇出: 同一字节进 A (shell) 与 B (用户态), 各自独立读写位
    mov ecx, [rel kb_head]
    lea eax, [rcx + 1]
    and eax, 63
    cmp eax, [rel kb_tail]
    je .doneA               ; A 满: 丢
    mov [rel kb_buf + rcx], dl
    mov [rel kb_head], eax
.doneA:
    mov ecx, [rel kb_head2]
    lea eax, [rcx + 1]
    and eax, 63
    cmp eax, [rel kb_tail2]
    je .done                ; B 满: 丢
    mov [rel kb_buf2 + rcx], dl
    mov [rel kb_head2], eax

.done:
    mov al, 0x20
    out 0x20, al            ; EOI (主 PIC)
    pop rdx
    pop rcx
    pop rax
    iretq

; ---------------- kbdrv_getc: 阻塞取一个字符 (shell 用) ----------------
; 返回 al = 字符
kbdrv_getc:
    push rcx
.wait:
    mov ecx, [rel kb_tail]
    cmp ecx, [rel kb_head]
    je .wait                ; 空: 自旋 (与 keyboard_read 轮询语义一致)
    mov ecx, [rel kb_tail]
    mov al, [rel kb_buf + rcx]
    inc ecx
    and ecx, 63
    mov [rel kb_tail], ecx
    pop rcx
    ret

; ---------------- kbdrv_take: 批量取 (SYS_READ 用, 非阻塞, 队列 B) ----------------
; rdi = 目的缓冲, rsi = 最大长度 → rax = 实际取到字节数
kbdrv_take:
    xor rax, rax
.loop:
    cmp rax, rsi
    jae .empty
    mov ecx, [rel kb_tail2]
    cmp ecx, [rel kb_head2]
    je .empty
    mov dl, [rel kb_buf2 + rcx]
    mov [rdi + rax], dl
    inc ecx
    and ecx, 63
    mov [rel kb_tail2], ecx
    inc rax
    jmp .loop
.empty:
    ret
