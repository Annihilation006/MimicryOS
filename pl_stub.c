#include "pl_readline.h"

void* kmalloc(unsigned int size);
void kfree(void* ptr);

void* malloc(unsigned long size) { return kmalloc((unsigned int)size); }
void free(void* ptr) { kfree(ptr); }

unsigned long strlen(const char* s) {
    unsigned long n = 0;
    while (s[n]) n++;
    return n;
}

char* strcpy(char* d, const char* s) {
    char* r = d;
    while ((*d++ = *s++));
    return r;
}

char* strcat(char* d, const char* s) {
    char* r = d;
    while (*d) d++;
    while ((*d++ = *s++));
    return r;
}

void* memset(void* dest, int val, unsigned long count) {
    char* d = dest;
    for (unsigned long i = 0; i < count; i++) d[i] = val;
    return dest;
}

void* memmove(void* dest, const void* src, unsigned long n) {
    char* d = dest;
    const char* s = src;
    if (d < s) {
        for (unsigned long i = 0; i < n; i++) d[i] = s[i];
    } else {
        for (unsigned long i = n; i > 0; i--) d[i-1] = s[i-1];
    }
    return dest;
}

char* strdup(const char* s) {
    unsigned long len = 0;
    while (s[len]) len++;
    char* d = (char*)kmalloc(len + 1);
    for (unsigned long i = 0; i <= len; i++) d[i] = s[i];
    return d;
}

int strncmp(const char* s1, const char* s2, unsigned long n) {
    while (n && *s1 && *s2 && *s1 == *s2) {
        s1++; s2++; n--;
    }
    if (n == 0) return 0;
    return *s1 - *s2;
}

char* strtok(char* s, const char* delim) {
    static char* last = 0;
    if (s) last = s;
    if (!last) return 0;
    while (*last) {
        int is_delim = 0;
        for (const char* d = delim; *d; d++) {
            if (*last == *d) { is_delim = 1; break; }
        }
        if (!is_delim) break;
        last++;
    }
    if (!*last) return 0;
    char* start = last;
    while (*last) {
        int is_delim = 0;
        for (const char* d = delim; *d; d++) {
            if (*last == *d) { is_delim = 1; break; }
        }
        if (is_delim) { *last = 0; last++; return start; }
        last++;
    }
    return start;
}

int vsprintf(char* buf, const char* fmt, __builtin_va_list ap) {
    char* p = buf;
    while (*fmt) {
        if (*fmt == '%') {
            fmt++;
            if (*fmt == 'z' && (*(fmt+1) == 'u' || *(fmt+1) == 'd')) fmt++;
            if (*fmt == 'd' || *fmt == 'u') {
                int v = __builtin_va_arg(ap, int);
                char tmp[16]; int i = 0;
                if (v == 0) tmp[i++] = '0';
                while (v > 0) { tmp[i++] = '0' + v % 10; v /= 10; }
                while (i > 0) *p++ = tmp[--i];
                fmt++;
            } else if (*fmt == 'c') {
                *p++ = (char)__builtin_va_arg(ap, int);
                fmt++;
            } else if (*fmt == 's') {
                char* s = __builtin_va_arg(ap, char*);
                while (*s) *p++ = *s++;
                fmt++;
            } else {
                *p++ = '%';
            }
        } else {
            *p++ = *fmt++;
        }
    }
    *p = '\0';
    return p - buf;
}

int sprintf(char* buf, const char* fmt, ...) {
    __builtin_va_list ap;
    __builtin_va_start(ap, fmt);
    int r = vsprintf(buf, fmt, ap);
    __builtin_va_end(ap);
    return r;
}
