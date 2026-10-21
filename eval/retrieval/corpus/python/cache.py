---
language: python
---
class LruCache:
    """Keep recently used values and evict the least recently used entry."""

    def __init__(self, capacity):
        self.capacity = capacity
        self.values = {}

    def get(self, key):
        value = self.values.pop(key, None)
        if value is not None:
            self.values[key] = value
        return value
