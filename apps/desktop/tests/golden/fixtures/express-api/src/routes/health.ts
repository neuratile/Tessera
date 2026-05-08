import { Router, type Request, type Response } from 'express';

import type { AuthService } from '../services/auth-service';

export function healthRouter(authService: AuthService): Router {
  const router = Router();

  router.get('/', (_req: Request, res: Response) => {
    res.status(200).json({
      status: 'ok',
      activeSessions: authService.activeSessionCount(),
    });
  });

  return router;
}
