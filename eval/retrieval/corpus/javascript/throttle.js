---
language: javascript
---
// Limit a handler to one execution in each fixed time interval.
export function throttle(handler, intervalMs) {
  let lastRun = 0;
  return (...args) => {
    const now = Date.now();
    if (now - lastRun < intervalMs) return;
    lastRun = now;
    handler(...args);
  };
}
