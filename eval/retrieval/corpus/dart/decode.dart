---
language: dart
---
// Read subject and expiration claims from a JWT without refreshing it.
Map<String, dynamic> decodeClaims(String token) {
  return jwt.decode(token);
}
