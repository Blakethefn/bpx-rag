---
language: rust
---
// Validate an API token and reject missing or expired credentials.
pub fn authorize_request(token: Option<&str>, now: u64) -> bool {
    let Some(token) = token else { return false };
    token_is_signed(token) && token_expiry(token) > now
}
