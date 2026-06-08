import {
  type TrackerConfigView,
  TrackerConfigViewSchema,
  type ExternalLink,
  ExternalLinkSchema,
  type PushResult,
  PushResultSchema,
  type BulkPushResultItem,
  BulkPushResultItemSchema,
} from '@testing-ide/shared';
import { z } from 'zod';

import { invokeAndParse, invokeString, invokeVoid } from './invoke';

export type SaveTrackerConfigArgs = {
  tracker: string;
  siteUrl: string;
  email: string;
  apiToken?: string | undefined;
  projectKey: string;
  issueType: string;
  isActive: boolean;
};

export type TestTrackerConnectionArgs = {
  tracker: string;
  siteUrl: string;
  email: string;
  apiToken?: string | undefined;
};

/** Save or update a tracker config. Backend returns the row id. */
export async function saveTrackerConfig(args: SaveTrackerConfigArgs): Promise<string> {
  return invokeString('save_tracker_config', { args });
}

/** List all tracker configs (tokens masked). */
export async function listTrackerConfigs(): Promise<TrackerConfigView[]> {
  return invokeAndParse('list_tracker_configs', z.array(TrackerConfigViewSchema), {});
}

/**
 * Fetch the single config for a given tracker, if any. There is no dedicated
 * Rust `get` command — we list and filter (the list is tiny: one row per
 * tracker, enforced by the `UNIQUE (user_id, tracker)` constraint).
 */
export async function getTrackerConfig(tracker = 'jira'): Promise<TrackerConfigView | null> {
  const configs = await listTrackerConfigs();
  return configs.find((c) => c.tracker === tracker) ?? null;
}

/** Delete a tracker config by its row id (UUID). */
export async function deleteTrackerConfig(id: string): Promise<void> {
  return invokeVoid('delete_tracker_config', { id });
}

/** Test a tracker connection. Backend returns the connected user's display name. */
export async function testTrackerConnection(args: TestTrackerConnectionArgs): Promise<string> {
  return invokeString('test_tracker_connection', { args });
}

/** Push a single artifact to Jira. Returns the created issue keys + urls. */
export async function pushArtifactToJira(artifactId: string): Promise<PushResult> {
  return invokeAndParse('push_to_tracker', PushResultSchema, { artifactId });
}

/** Push many artifacts to Jira; failures are reported per-item, not fatal. */
export async function bulkPushArtifactsToJira(artifactIds: string[]): Promise<BulkPushResultItem[]> {
  return invokeAndParse('bulk_push_to_tracker', z.array(BulkPushResultItemSchema), { artifactIds });
}

/** Refresh a linked issue's status; returns the updated link row. */
export async function refreshExternalLinkStatus(linkId: string): Promise<ExternalLink> {
  return invokeAndParse('refresh_tracker_link_status', ExternalLinkSchema, { linkId });
}

/**
 * List external links. Pass an `artifactId` to scope to one artifact, or omit
 * it to list every link (used by the AI panel to build its artifact→link map).
 */
export async function listExternalLinks(artifactId?: string): Promise<ExternalLink[]> {
  return invokeAndParse('list_external_links', z.array(ExternalLinkSchema), { artifactId });
}
