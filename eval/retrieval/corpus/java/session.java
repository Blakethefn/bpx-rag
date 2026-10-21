---
language: java
---
// Extend an active browser session when a user performs an action.
public Session touch(Session session, Instant now) {
    return session.withLastActivity(now);
}
