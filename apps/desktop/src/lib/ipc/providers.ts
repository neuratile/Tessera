import {
  type OllamaModel,
  OllamaModelSchema,
  type ProviderConfigView,
  ProviderConfigViewSchema,
  type ProviderConnectionTestArgs,
  ProviderConnectionTestArgsSchema,
  type ProviderConnectionTestResult,
  ProviderConnectionTestResultSchema,
  type SaveProviderArgs,
  SaveProviderArgsSchema,
} from '@testing-ide/shared';
import { z } from 'zod';

import { IpcError } from './error';
import { invokeAndParse, invokeString, invokeVoid } from './invoke';

const ProviderConfigViewListSchema = z.array(ProviderConfigViewSchema);

/**
 * Save / upsert a provider config. The Tauri command returns the row id
 * as a plain string (`Result<String, String>`), not a JSON body.
 */
export async function saveProviderConfig(args: SaveProviderArgs): Promise<string> {
  const parsed = SaveProviderArgsSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError('save_provider_config', `invalid arguments: ${parsed.error.message}`);
  }
  return invokeString('save_provider_config', { args: parsed.data });
}

export async function listProviderConfigs(): Promise<ProviderConfigView[]> {
  return invokeAndParse('list_provider_configs', ProviderConfigViewListSchema);
}

export async function deleteProviderConfig(id: string): Promise<void> {
  return invokeVoid('delete_provider_config', { id });
}

const OllamaModelListSchema = z.array(OllamaModelSchema);

export async function listOllamaModels(baseUrl?: string): Promise<OllamaModel[]> {
  const args: Record<string, unknown> = {};
  if (typeof baseUrl === 'string' && baseUrl.length > 0) {
    args.baseUrl = baseUrl;
  }
  return invokeAndParse('list_ollama_models', OllamaModelListSchema, args);
}

export async function testProviderConnection(
  args: ProviderConnectionTestArgs,
): Promise<ProviderConnectionTestResult> {
  const parsed = ProviderConnectionTestArgsSchema.safeParse(args);
  if (!parsed.success) {
    throw new IpcError(
      'test_provider_connection',
      `invalid arguments: ${parsed.error.message}`,
    );
  }
  return invokeAndParse('test_provider_connection', ProviderConnectionTestResultSchema, {
    args: parsed.data,
  });
}
