---
language: dart
---
// Renew an expired access token using the saved refresh credential.
Future<Token> refreshSession(Session session) async {
  final response = await api.refresh(session.refreshToken);
  await sessions.save(session.copyWith(token: response.accessToken));
  return response;
}
