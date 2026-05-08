import { mockIPC } from '@tauri-apps/api/mocks';
import type {
  AnalysisOutcome,
  ArtifactDetail,
  ArtifactSummary,
  GenerateResponse,
  HardwareInfo,
  HealthStatus,
  OllamaStatus,
  Project,
  ProviderConfigView,
} from '@testing-ide/shared';

import {
  getTestingIdeE2eConfig,
  getTestingIdeReadDir,
  getTestingIdeReadTextFile,
  getTestingIdeStat,
} from './e2e-bridge';

const MOCK_USER_ID = '00000000-0000-4000-8000-000000000001';
const MOCK_PROJECT_ID = '11111111-1111-4111-8111-111111111111';
const MOCK_PROVIDER_ID = '22222222-2222-4222-8222-222222222222';
const MOCK_ARTIFACT_ID = '33333333-3333-4333-8333-333333333333';

type GeneratePayload = {
  args?: {
    projectId: string;
    projectName: string;
    artifactType: string;
    model: string;
    provider: string;
    parentId?: string;
  };
};

type MockArtifactState = {
  summary: ArtifactSummary;
  detail: ArtifactDetail;
};

function isoNow(): string {
  return new Date().toISOString();
}

function buildProject(rootPath: string, status: Project['status']): Project {
  const timestamp = isoNow();

  return {
    id: MOCK_PROJECT_ID,
    name: 'express-api',
    rootPath,
    fileCount: status === 'ready' ? 7 : 0,
    totalSizeBytes: status === 'ready' ? 2_048 : 0,
    status,
    languageBreakdown: status === 'ready' ? { typescript: 4 } : {},
    createdAt: timestamp,
    updatedAt: timestamp,
  };
}

function buildAnalysisOutcome(): AnalysisOutcome {
  return {
    projectId: MOCK_PROJECT_ID,
    filesDiscovered: 7,
    filesParsed: 4,
    chunksCreated: 9,
    chunksEmbedded: 9,
    totalSizeBytes: 2_048,
  };
}

function buildProviderConfig(): ProviderConfigView {
  return {
    id: MOCK_PROVIDER_ID,
    provider: 'ollama',
    hasApiKey: false,
    baseUrl: 'http://localhost:11434',
    defaultModel: 'qwen2.5-coder:7b',
    isActive: true,
  };
}

function buildHealthStatus(): HealthStatus {
  return {
    dbOk: true,
    osName: 'Windows',
    osVersion: '11',
    totalMemoryMb: 32_768,
    availableMemoryMb: 24_576,
    cpuCount: 16,
  };
}

function buildHardwareInfo(): HardwareInfo {
  return {
    ramGb: 32,
    gpuName: 'NVIDIA GeForce RTX 4090',
    gpuVramGb: 24,
    recommendedModel: 'qwen2.5-coder:32b',
  };
}

function buildOllamaStatus(): OllamaStatus {
  return {
    installed: true,
    running: true,
    models: ['qwen2.5-coder:7b', 'qwen2.5-coder:32b', 'nomic-embed-text'],
  };
}

function buildTestPlanArtifact(projectName: string): MockArtifactState {
  const timestamp = isoNow();
  const title = `Test Plan - ${projectName}`;
  const contentMd = `# Test Plan\n\n## Summary\n\nCovers the ${projectName} auth and health flows.\n\n## Objectives\n\n- Verify login and logout behavior.\n- Confirm health endpoint availability.\n`;

  return {
    summary: {
      id: MOCK_ARTIFACT_ID,
      projectId: MOCK_PROJECT_ID,
      artifactType: 'test-plan',
      title,
      status: 'draft',
      version: 1,
      parentId: null,
      createdAt: timestamp,
      updatedAt: timestamp,
      provider: 'ollama',
      model: 'qwen2.5-coder:7b',
    },
    detail: {
      id: MOCK_ARTIFACT_ID,
      projectId: MOCK_PROJECT_ID,
      artifactType: 'test-plan',
      title,
      contentMd,
      structuredData: {
        summary: 'Validate the Express API auth and health flows.',
        objectives: ['Verify login', 'Verify logout', 'Verify health checks'],
        scopeIn: ['POST /auth/login', 'POST /auth/logout', 'GET /health'],
        scopeOut: ['Database migrations'],
        strategy: ['API-level happy path coverage'],
        environments: ['local'],
        risks: [{ description: 'Session regressions', mitigation: 'Review auth outputs' }],
        entryCriteria: ['Project uploaded'],
        exitCriteria: ['Review completed'],
      },
      status: 'draft',
      version: 1,
      parentId: null,
      createdAt: timestamp,
      updatedAt: timestamp,
      provider: 'ollama',
      model: 'qwen2.5-coder:7b',
      promptVersion: 'test-plan-v1',
      inputTokens: 256,
      outputTokens: 192,
    },
  };
}

async function readDirEntries(path: string) {
  const readDir = getTestingIdeReadDir();
  if (readDir === null) {
    throw new Error('E2E readDir bridge is not installed');
  }

  return readDir(path);
}

async function readTextFile(path: string): Promise<number[]> {
  const readFile = getTestingIdeReadTextFile();
  if (readFile === null) {
    throw new Error('E2E readTextFile bridge is not installed');
  }

  const text = await readFile(path);
  return Array.from(new TextEncoder().encode(text));
}

async function statFile(path: string) {
  const stat = getTestingIdeStat();
  if (stat === null) {
    throw new Error('E2E stat bridge is not installed');
  }

  const metadata = await stat(path);
  return {
    isFile: true,
    isDirectory: false,
    isSymlink: false,
    size: metadata.size,
    mtime: null,
    atime: null,
    birthtime: null,
    readonly: false,
    mode: null,
    uid: null,
    gid: null,
    dev: null,
    ino: null,
    nlink: null,
    blksize: null,
    blocks: null,
    rdev: null,
  };
}

export function installE2eTauriMocks(): void {
  const config = getTestingIdeE2eConfig();
  if (config === null) {
    return;
  }

  let project: Project | null = null;
  let artifact: MockArtifactState | null = null;
  const provider = buildProviderConfig();

  mockIPC(async (command, payload) => {
    switch (command) {
      case 'plugin:dialog|open':
        return config.fixtureRoot;

      case 'plugin:dialog|save':
        return config.exportFilePath;

      case 'plugin:fs|read_dir': {
        const args = payload as { path?: string };
        if (typeof args.path !== 'string' || args.path.length === 0) {
          throw new Error('missing read_dir path');
        }

        const entries = await readDirEntries(args.path);
        return entries.map((entry) => ({
          name: entry.name,
          isDirectory: entry.isDirectory,
        }));
      }

      case 'plugin:fs|read_text_file': {
        const args = payload as { path?: string };
        if (typeof args.path !== 'string' || args.path.length === 0) {
          throw new Error('missing read_text_file path');
        }

        return readTextFile(args.path);
      }

      case 'plugin:fs|stat': {
        const args = payload as { path?: string };
        if (typeof args.path !== 'string' || args.path.length === 0) {
          throw new Error('missing stat path');
        }

        return statFile(args.path);
      }

      case 'health_check':
        return buildHealthStatus();

      case 'detect_hardware':
        return buildHardwareInfo();

      case 'check_ollama_status':
        return buildOllamaStatus();

      case 'create_project': {
        const args = payload as { rootPath?: string };
        if (typeof args.rootPath !== 'string' || args.rootPath.length === 0) {
          throw new Error('missing rootPath');
        }

        project = buildProject(args.rootPath, 'pending');
        artifact = null;
        return project;
      }

      case 'get_project':
        if (project === null) {
          throw new Error('project not found');
        }
        return project;

      case 'list_projects':
        return project === null ? [] : [project];

      case 'delete_project':
        project = null;
        artifact = null;
        return null;

      case 'analyze_project':
        if (project === null) {
          throw new Error('project not found');
        }
        project = {
          ...project,
          status: 'ready',
          fileCount: 7,
          totalSizeBytes: 2_048,
          languageBreakdown: { typescript: 4 },
          updatedAt: isoNow(),
        };
        return buildAnalysisOutcome();

      case 'list_provider_configs':
        return [provider];

      case 'generate_artifact': {
        if (project === null) {
          throw new Error('project not found');
        }

        const args = (payload as GeneratePayload).args;
        if (args?.artifactType !== 'test-plan') {
          throw new Error('E2E mock only supports test-plan generation');
        }

        artifact = buildTestPlanArtifact(args.projectName);
        const response: GenerateResponse = {
          generationId: '00000000-0000-4000-8000-000000000001',
          artifactId: artifact.detail.id,
          artifactType: artifact.detail.artifactType,
          contentMd: artifact.detail.contentMd,
          usageInputTokens: artifact.detail.inputTokens,
          usageOutputTokens: artifact.detail.outputTokens,
        };
        return response;
      }

      case 'list_artifacts':
        return artifact === null ? [] : [artifact.summary];

      case 'get_artifact': {
        const args = payload as { id?: string };
        if (artifact === null || args.id !== artifact.detail.id) {
          throw new Error('artifact not found');
        }
        return artifact.detail;
      }

      case 'approve_artifact': {
        const args = payload as { id?: string };
        if (artifact === null || args.id !== artifact.detail.id) {
          throw new Error('artifact not found');
        }

        const updatedAt = isoNow();
        artifact = {
          summary: { ...artifact.summary, status: 'approved', updatedAt },
          detail: { ...artifact.detail, status: 'approved', updatedAt },
        };
        return null;
      }

      case 'reject_artifact': {
        const args = payload as { id?: string };
        if (artifact === null || args.id !== artifact.detail.id) {
          throw new Error('artifact not found');
        }

        const updatedAt = isoNow();
        artifact = {
          summary: { ...artifact.summary, status: 'rejected', updatedAt },
          detail: { ...artifact.detail, status: 'rejected', updatedAt },
        };
        return null;
      }

      case 'login':
      case 'register':
      case 'refresh_token':
        return {
          accessToken: MOCK_USER_ID,
          refreshToken: MOCK_USER_ID,
          tokenType: 'Bearer',
        };

      case 'auth_me':
        return {
          id: MOCK_USER_ID,
          email: 'e2e@example.com',
          name: 'E2E User',
        };

      default:
        throw new Error(`Unhandled mocked Tauri command: ${command}`);
    }
  });
}
