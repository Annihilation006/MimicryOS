#ifndef _STDLIB_H
#define _STDLIB_H
/* 供 clang --target=x86_64-none-elf 的 freestanding 环境使用。
 * stb_image.h 会无条件 #include <stdlib.h>，但该目标没有 libc 头文件；
 * 又因定义了 STBI_NO_STDLIB，stb 实际只用到底层类型与 NULL。 */
typedef unsigned long size_t;
#define NULL ((void*)0)
#endif
