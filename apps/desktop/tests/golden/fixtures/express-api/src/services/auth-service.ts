import crypto from 'node:crypto';

type UserRecord = {
  id: string;
  email: string;
  password: string;
  displayName: string;
};

type LoginResponse = {
  sessionToken: string;
  user: {
    id: string;
    email: string;
    displayName: string;
  };
};

class SessionStore {
  private readonly sessions = new Map<string, string>();

  createSession(userId: string): string {
    const token = crypto.randomUUID();
    this.sessions.set(token, userId);
    return token;
  }

  revokeSession(token: string): boolean {
    return this.sessions.delete(token);
  }

  activeSessionCount(): number {
    return this.sessions.size;
  }
}

const USERS: UserRecord[] = [
  {
    id: 'user-1',
    email: 'qa@example.com',
    password: 'secret123',
    displayName: 'QA User',
  },
  {
    id: 'user-2',
    email: 'admin@example.com',
    password: 'admin123',
    displayName: 'Admin User',
  },
];

export type AuthService = {
  login(email: string, password: string): Promise<LoginResponse>;
  logout(token: string): Promise<boolean>;
  activeSessionCount(): number;
};

export function createAuthService(): AuthService {
  const sessionStore = new SessionStore();

  async function login(email: string, password: string): Promise<LoginResponse> {
    if (email.trim() === '' || password.trim() === '') {
      throw new Error('email and password are required');
    }

    const user = USERS.find((candidate) => candidate.email === email);
    if (user === undefined || user.password !== password) {
      throw new Error('invalid credentials');
    }

    const sessionToken = sessionStore.createSession(user.id);
    return {
      sessionToken,
      user: {
        id: user.id,
        email: user.email,
        displayName: user.displayName,
      },
    };
  }

  async function logout(token: string): Promise<boolean> {
    if (token.trim() === '') {
      return false;
    }

    return sessionStore.revokeSession(token);
  }

  function activeSessionCount(): number {
    return sessionStore.activeSessionCount();
  }

  return {
    login,
    logout,
    activeSessionCount,
  };
}
