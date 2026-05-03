import { z } from 'zod';

const IsoDateTimeSchema = z.string().datetime({ offset: true });

/**
 * Public user record returned by the API (no secrets).
 */
export const UserSchema = z.object({
  id: z.string().uuid(),
  email: z.string().email(),
  name: z.string().min(1),
  plan: z.string().min(1),
  createdAt: IsoDateTimeSchema,
  updatedAt: IsoDateTimeSchema,
});

export type User = z.infer<typeof UserSchema>;

/**
 * Full user row shape for server-side persistence (includes password hash).
 * Never return this from public HTTP responses.
 */
export const UserRecordSchema = UserSchema.extend({
  passwordHash: z.string().min(1),
});

export type UserRecord = z.infer<typeof UserRecordSchema>;
