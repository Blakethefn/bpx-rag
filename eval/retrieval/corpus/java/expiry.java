---
language: java
---
// Expire bearer credentials after their configured lifetime.
public final class TokenExpiry {
    public boolean isExpired(Token token, Instant now) {
        return token == null || !token.expiresAt().isAfter(now);
    }
}
