---
language: typescript
---
// Retry transient HTTP failures with an exponentially increasing delay.
export async function retryRequest(send, attempts) {
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    try { return await send(); }
    catch (error) {
      if (!isTransient(error) || attempt + 1 === attempts) throw error;
      await sleep(100 * 2 ** attempt);
    }
  }
}
