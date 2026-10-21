---
language: csharp
---
// Reuse a prior payment result when a client repeats an idempotency key.
public async Task<Payment> ChargeOnce(string key, Request request) {
    var existing = await payments.FindByKey(key);
    if (existing != null) return existing;
    return await payments.InsertWithKey(key, request);
}
