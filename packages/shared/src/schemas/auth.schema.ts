import { z } from 'zod';

/**
 * Registration request body (API boundary).
 */
export const RegisterSchema = z.object({
  email: z.string().email(),
  password: z.string().min(8).max(256),
  name: z.string().min(1).max(200).optional(),
});

export type RegisterInput = z.infer<typeof RegisterSchema>;

/**
 * Login request body (API boundary).
 */
export const LoginSchema = z.object({
  email: z.string().email(),
  password: z.string().min(1).max(256),
});

export type LoginInput = z.infer<typeof LoginSchema>;

/**
 * JWT access token payload (after verification, before app claims).
 */
export const JWTPayloadSchema = z.object({
  sub: z.string().uuid(),
  email: z.string().email(),
  iat: z.number().int().nonnegative(),
  exp: z.number().int().nonnegative(),
  jti: z.string().min(1).optional(),
});

export type JWTPayload = z.infer<typeof JWTPayloadSchema>;
