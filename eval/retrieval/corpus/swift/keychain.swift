---
language: swift
---
// Store refreshed credentials in the iOS Keychain, not ordinary preferences.
func saveRefreshToken(_ token: String, account: String) throws {
    try keychain.set(token, key: "refresh-token-\(account)")
}
