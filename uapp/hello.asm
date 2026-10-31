; uapp/hello.asm — 最小 ring3 用户程序 (NASM -f bin 裸二进制)
;
; 装载 ABI (与 elfprog 包装器对齐):
;   本文件汇编成纯代码段, 由包装器加上 120 字节 ELF 头+程序头后
;   装到 base, 代码落在 base+120, 入口 = base+120 (即本文件 start)。
;   RIP 相对寻址与装载基址无关, 包装头平移不影响本文件。
;
; 行为: write(2, "HELLO FROM ASM") 画到行 14, 然后 exit(0)。
; 系统调用 ABI (设计 07): rax=号, rdi=fd, rsi/rdx=缓冲/长度, int 0x80。

BITS 64

start:
    mov eax, 1                  ; SYS_WRITE
    mov edi, 2                  ; fd 2 → 行 14
    lea rsi, [rel msg]          ; 消息在本段内, RIP 相对即位置无关
    mov edx, msg.len
    int 0x80
    mov eax, 60                 ; SYS_EXIT
    int 0x80

msg: db "HELLO FROM ASM"
.len equ $ - msg
