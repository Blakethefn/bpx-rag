# Retry policy

Retry only temporary network failures. Stop after the configured attempt limit
and return permanent errors without retrying them.

Use exponential backoff between attempts: the delay grows after each transient
failure so retries do not immediately add more pressure to an unhealthy service.
Keep the retry schedule bounded by the caller's maximum attempt count.
