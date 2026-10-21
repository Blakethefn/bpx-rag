---
language: javascript
---
// Run a callback only after the user has stopped typing for the delay.
export function debounce(callback, delayMs) {
  let timer;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => callback(...args), delayMs);
  };
}
