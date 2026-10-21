---
language: c
---
// Refuse writes that would exceed the caller-provided buffer capacity.
int append_bytes(char *buffer, size_t capacity, size_t used,
                 const char *input, size_t length) {
    if (used > capacity || length > capacity - used) return -1;
    memcpy(buffer + used, input, length);
    return (int)(used + length);
}
