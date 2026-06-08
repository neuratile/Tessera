import { z } from 'zod';

/**
 * Mirrors `tracker_config_service::TrackerConfigView` (serde `camelCase`).
 * The API token is never sent to the client — only a `hasApiToken` flag.
 */
export const TrackerConfigViewSchema = z.object({
  id: z.string().uuid(),
  tracker: z.string(),
  siteUrl: z.string(),
  email: z.string(),
  hasApiToken: z.boolean(),
  projectKey: z.string(),
  issueType: z.string(),
  severityMapJson: z.string().nullable().optional(),
  isActive: z.boolean(),
});

export type TrackerConfigView = z.infer<typeof TrackerConfigViewSchema>;

/** Mirrors `external_link_repo::ExternalLinkRow` (serde `camelCase`). */
export const ExternalLinkSchema = z.object({
  id: z.string().uuid(),
  artifactId: z.string().uuid(),
  tracker: z.string(),
  itemRef: z.string(),
  issueKey: z.string(),
  issueUrl: z.string(),
  issueType: z.string().nullable().optional(),
  lastStatus: z.string().nullable().optional(),
  statusFetchedAt: z.string().nullable().optional(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export type ExternalLink = z.infer<typeof ExternalLinkSchema>;

/**
 * Mirrors `jira_push_service::PushResult` (serde `camelCase`).
 * One artifact can produce many issues (e.g. each test case), so keys/urls
 * are parallel arrays.
 */
export const PushResultSchema = z.object({
  keys: z.array(z.string()),
  urls: z.array(z.string()),
});

export type PushResult = z.infer<typeof PushResultSchema>;

/** Mirrors `jira_push_service::BulkPushResultItem` (serde `camelCase`). */
export const BulkPushResultItemSchema = z.object({
  artifactId: z.string(),
  success: z.boolean(),
  keys: z.array(z.string()),
  error: z.string().nullable().optional(),
});

export type BulkPushResultItem = z.infer<typeof BulkPushResultItemSchema>;
