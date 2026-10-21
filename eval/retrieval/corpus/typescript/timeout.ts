---
language: typescript
---
// Abort a request that runs longer than the configured timeout.
export function timeoutSignal(timeoutMs) {
  const controller = new AbortController();
  setTimeout(() => controller.abort(), timeoutMs);
  return controller.signal;
}
