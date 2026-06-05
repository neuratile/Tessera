import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { z } from 'zod';

export const DEFAULT_OLLAMA_BASE_URL = 'http://localhost:11434';
export const DEFAULT_EMBED_MODEL = 'nomic-embed-text';
export const CHAT_MODEL_FALLBACKS = ['qwen2.5-coder:7b', 'qwen2.5-coder:1.5b'] as const;
export const HTTP_TIMEOUT_MS = 120_000;
// 15 min per cargo probe attempt. The golden test-cases probe reserves
// 6k output tokens (parity with the desktop app — the runnable `files[]`
// payload pushed the cases-only 4k budget into truncation) and a warmed
// 3B model on the 2-vCPU CI runner streams that in well under the cap;
// the margin absorbs a cold-load stall without turning slow runs into
// timeouts.
export const PROCESS_TIMEOUT_MS = 900_000;
const JSON_RESULT_PREFIX = 'JSON_RESULT:';

export const desktopRoot = fileURLToPath(new URL('../../', import.meta.url));

const OllamaVersionSchema = z.object({
  version: z.string().min(1),
});

const OllamaTagsSchema = z.object({
  models: z.array(
    z.object({
      name: z.string().min(1),
    }),
  ),
});

export type SelectedModel = {
  requested: string;
  installed: string;
};

export type ReadyOllamaIntegrationContext = {
  ready: true;
  baseUrl: string;
  models: string[];
  chatModel: SelectedModel;
  embedModel: SelectedModel | null;
};

export type OllamaIntegrationContext =
  | ReadyOllamaIntegrationContext
  | {
      ready: false;
      reason: string;
    };

export type ResolveOllamaIntegrationOptions = {
  requireEmbedding?: boolean;
};

export function normalizeBaseUrl(raw: string): string {
  return raw.trim().replace(/\/+$/, '');
}

export function isModelMatch(requested: string, installed: string): boolean {
  if (installed === requested) {
    return true;
  }

  if (!installed.startsWith(requested)) {
    return false;
  }

  const suffix = installed.slice(requested.length);
  return suffix.startsWith(':') || suffix.startsWith('-');
}

function resolveRequestedChatModel(): string | null {
  const explicit = process.env.OLLAMA_TEST_CHAT_MODEL?.trim();
  if (explicit && explicit.length > 0) {
    return explicit;
  }

  return null;
}

function resolveRequestedEmbedModel(): string {
  const explicit = process.env.OLLAMA_TEST_EMBED_MODEL?.trim();
  if (explicit && explicit.length > 0) {
    return explicit;
  }

  return DEFAULT_EMBED_MODEL;
}

function selectInstalledModel(
  installedModels: readonly string[],
  requested: string | null,
  fallbacks: readonly string[],
): SelectedModel | null {
  const candidates = requested === null ? [...fallbacks] : [requested];

  for (const candidate of candidates) {
    const installed = installedModels.find((model) => isModelMatch(candidate, model));
    if (installed !== undefined) {
      return { requested: candidate, installed };
    }
  }

  return null;
}

export function buildUrl(baseUrl: string, pathname: string): URL {
  const normalized = baseUrl.endsWith('/') ? baseUrl : `${baseUrl}/`;
  return new URL(pathname.replace(/^\//, ''), normalized);
}

export async function fetchJson<Output>(
  url: URL,
  schema: z.ZodType<Output>,
  init?: RequestInit,
): Promise<Output> {
  const controller = new AbortController();
  const timeout = setTimeout(() => {
    controller.abort();
  }, HTTP_TIMEOUT_MS);

  try {
    const response = await fetch(url, {
      ...init,
      signal: controller.signal,
    });

    if (!response.ok) {
      throw new Error(`request to ${url} failed with status ${response.status}`);
    }

    const responseJson: unknown = await response.json();
    const parsed = schema.safeParse(responseJson);
    if (!parsed.success) {
      throw new Error(`response from ${url} failed schema validation: ${parsed.error.message}`);
    }

    return parsed.data;
  } finally {
    clearTimeout(timeout);
  }
}

async function commandExists(command: string): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    const child = spawn(command, ['--version'], {
      stdio: 'ignore',
      windowsHide: true,
    });

    child.once('error', () => {
      resolve(false);
    });
    child.once('close', (code) => {
      resolve(code === 0);
    });
  });
}

export async function resolveIntegrationContext(
  options: ResolveOllamaIntegrationOptions = {},
): Promise<OllamaIntegrationContext> {
  const requireEmbedding = options.requireEmbedding ?? true;
  const baseUrl = normalizeBaseUrl(process.env.OLLAMA_BASE_URL ?? DEFAULT_OLLAMA_BASE_URL);

  if (!(await commandExists('cargo'))) {
    return {
      ready: false,
      reason: 'cargo is not available in PATH, so the Rust probe binaries cannot run.',
    };
  }

  try {
    await fetchJson(buildUrl(baseUrl, '/api/version'), OllamaVersionSchema);
  } catch (error) {
    return {
      ready: false,
      reason: `Ollama is not reachable at ${baseUrl}: ${error instanceof Error ? error.message : String(error)}`,
    };
  }

  const tags = await fetchJson(buildUrl(baseUrl, '/api/tags'), OllamaTagsSchema);
  const installedModels = tags.models.map((model) => model.name);
  const requestedChatModel = resolveRequestedChatModel();
  const requestedEmbedModel = resolveRequestedEmbedModel();

  const chatModel = selectInstalledModel(installedModels, requestedChatModel, CHAT_MODEL_FALLBACKS);
  if (chatModel === null) {
    return {
      ready: false,
      reason:
        requestedChatModel === null
          ? `No supported qwen2.5-coder chat model is installed. Expected one of: ${CHAT_MODEL_FALLBACKS.join(', ')}`
          : `Requested chat model \`${requestedChatModel}\` is not installed in Ollama.`,
    };
  }

  const embedModel = selectInstalledModel(installedModels, requestedEmbedModel, [requestedEmbedModel]);
  if (requireEmbedding && embedModel === null) {
    return {
      ready: false,
      reason: `Requested embedding model \`${requestedEmbedModel}\` is not installed in Ollama.`,
    };
  }

  return {
    ready: true,
    baseUrl,
    models: installedModels,
    chatModel,
    embedModel,
  };
}

export async function runCargoJsonProbeTest<Output>(
  testName: string,
  schema: z.ZodType<Output>,
  envVars: Record<string, string>,
): Promise<Output> {
  return new Promise<Output>((resolve, reject) => {
    const stdoutChunks: Buffer[] = [];
    const stderrChunks: Buffer[] = [];
    const child = spawn(
      'cargo',
      [
        'test',
        '--manifest-path',
        'src-tauri/Cargo.toml',
        '--locked',
        '-j',
        '1',
        '--lib',
        testName,
        '--',
        '--ignored',
        '--exact',
        '--nocapture',
      ],
      {
        cwd: desktopRoot,
        stdio: ['ignore', 'pipe', 'pipe'],
        windowsHide: true,
        env: {
          ...process.env,
          ...envVars,
        },
      },
    );

    const timeout = setTimeout(() => {
      child.kill();
      reject(new Error(`${testName} timed out`));
    }, PROCESS_TIMEOUT_MS);

    child.stdout.on('data', (chunk: Buffer) => {
      stdoutChunks.push(chunk);
    });
    child.stderr.on('data', (chunk: Buffer) => {
      stderrChunks.push(chunk);
    });

    child.once('error', (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.once('close', (code) => {
      clearTimeout(timeout);

      if (code !== 0) {
        const stderr = Buffer.concat(stderrChunks).toString('utf8').trim();
        reject(
          new Error(
            stderr.length > 0
              ? `${testName} failed: ${stderr}`
              : `${testName} exited with code ${code ?? 'unknown'}`,
          ),
        );
        return;
      }

      const stdout = Buffer.concat(stdoutChunks).toString('utf8').trim();
      const jsonLine = stdout
        .split(/\r?\n/u)
        .find((line) => line.startsWith(JSON_RESULT_PREFIX));
      if (jsonLine === undefined) {
        reject(new Error(`${testName} did not print a ${JSON_RESULT_PREFIX} payload`));
        return;
      }

      let parsedJson: unknown;
      try {
        parsedJson = JSON.parse(jsonLine.slice(JSON_RESULT_PREFIX.length));
      } catch (error) {
        reject(
          new Error(
            `${testName} returned invalid JSON: ${
              error instanceof Error ? error.message : String(error)
            }`,
          ),
        );
        return;
      }

      const parsed = schema.safeParse(parsedJson);
      if (!parsed.success) {
        reject(new Error(`${testName} failed schema validation: ${parsed.error.message}`));
        return;
      }

      resolve(parsed.data);
    });
  });
}
