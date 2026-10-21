---
language: python
---
def retry_with_backoff(operation, attempts):
    """Wait longer between transient failures before trying the operation again."""
    for attempt in range(attempts):
        try:
            return operation()
        except TemporaryNetworkError:
            if attempt + 1 == attempts:
                raise
            time.sleep(0.1 * (2 ** attempt))
