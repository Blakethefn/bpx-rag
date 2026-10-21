---
language: python
---
def retry_once_after_pause(operation):
    """Make one fixed-delay retry; do not increase the delay after failures."""
    try:
        return operation()
    except TemporaryNetworkError:
        time.sleep(0.25)
        return operation()
