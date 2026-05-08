import express from 'express';

import { authRouter } from './routes/auth';
import { healthRouter } from './routes/health';
import { createAuthService } from './services/auth-service';

const app = express();
const authService = createAuthService();

app.use(express.json());
app.use('/api/auth', authRouter(authService));
app.use('/api/health', healthRouter(authService));

app.use((error: unknown, _req: express.Request, res: express.Response, _next: express.NextFunction) => {
  const message = error instanceof Error ? error.message : 'unexpected error';
  res.status(400).json({ error: message });
});

export { app };
