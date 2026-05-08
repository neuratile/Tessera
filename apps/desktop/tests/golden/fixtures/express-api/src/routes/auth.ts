import { Router, type Request, type Response } from 'express';

import type { AuthService } from '../services/auth-service';

export function authRouter(authService: AuthService): Router {
  const router = Router();

  router.post('/login', async (req: Request, res: Response) => {
    const email = typeof req.body?.email === 'string' ? req.body.email : '';
    const password = typeof req.body?.password === 'string' ? req.body.password : '';

    const result = await authService.login(email, password);
    res.status(200).json(result);
  });

  router.post('/logout', async (req: Request, res: Response) => {
    const token = req.header('x-session-token') ?? '';
    const revoked = await authService.logout(token);

    if (!revoked) {
      res.status(404).json({ revoked: false });
      return;
    }

    res.status(204).send();
  });

  return router;
}
