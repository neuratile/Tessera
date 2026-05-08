# Express Auth Fixture

Small TypeScript Express API used by the golden prompt tests.

Behavior:
- `POST /api/auth/login` validates email/password and returns a session token plus a public user profile.
- `POST /api/auth/logout` revokes the caller's current session token.
- `GET /api/health` reports service status and the number of active sessions.

Design notes:
- Session state is stored in memory through `SessionStore`.
- `createAuthService()` centralizes login, logout, and session-count behavior.
- The app has a single error-handling middleware that converts thrown errors into JSON responses.
