---
language: csharp
---
// Retry a failed payment call without checking for an earlier charge.
public async Task<Payment> RetryCharge(Request request) {
    try { return await gateway.Charge(request); }
    catch (NetworkException) { return await gateway.Charge(request); }
}
