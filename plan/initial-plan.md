# AI-Powered Testing IDE — Implementation Plan

## Context

Building a VS Code-like IDE focused exclusively on AI-powered software testing. Users upload project folders; AI analyzes code structure, data flows, architecture, then generates structured test artifacts (test plans, test cases, defect reports, bug reports, test summaries). Human reviews and approves. No existing code — clean slate at `C:\Testing IDE`.

**Differentiation**: No tool currently owns the "static code analysis → full test strategy" space. Copilot/Cursor generate test code snippets. Mabl/TestRigor need running apps. This IDE bridges the gap — architecture-aware, structured output, no execution required.

**Team**: Small team (2-3 people). No fixed deadline — quality over speed, ship incrementally. Backend language and deployment decisions deferred to implementation time.

---

## System Architecture

```
Frontend (React+Vite)  <-- REST + SSE -->  Backend (Express/Node)  <-->  PostgreSQL + pgvector
     |                                          |
  Monaco Editor                          Tree-sitter (AST)
  File Tree                              Claude API (LLM)
  AI Action Panel                        Voyage AI (embeddings, V1+)
  Markdown Preview                       Redis + BullMQ (V1+)
```

### MVP Approach
- Provider-agnostic LLM layer from day 1 (Ollama Local default + cloud providers via API key)
- **RAG included in MVP** — small local models (7B) have 32-128K context; RAG essential for any non-trivial project
- pgvector for embeddings storage
- Ollama for local embeddings (`nomic-embed-text`, runs anywhere)
- No Redis / no job queue initially (synchronous + SSE streaming)
- Single deployment artifact

---

## Tech Stack

| Layer | Technology | Reasoning |
|-------|-----------|-----------|
| Monorepo | pnpm + Turborepo | Fast installs, build caching |
| Frontend | React 19 + TypeScript + Vite + TailwindCSS v4 | Standard, fast DX |
| Editor | @monaco-editor/react | VS Code parity for free |
| File Tree | react-arborist | Virtualized, full-featured |
| Layout | react-resizable-panels | Three-panel IDE layout |
| State | Zustand | Minimal boilerplate |
| Backend | Node.js + Express + TS **OR** Python + FastAPI | Node: same lang as frontend, great streaming. Python: better ML libs, mature AST tooling. Decide at impl time. |
| ORM | Drizzle (Node) or SQLAlchemy (Python) | Both excellent, match backend choice |
| Database | PostgreSQL 16 + pgvector | Relational + vector in one DB |
| AST Parsing | web-tree-sitter (WASM) | 305 languages, incremental |
| LLM (provider-agnostic) | OpenAI / Anthropic / OpenRouter / Ollama (Cloud + Local) | User picks provider. Default: Ollama Local (qwen2.5-coder:7b) — free, no API key needed. See Model Strategy. |
| Embeddings | Ollama nomic-embed-text (local) OR Voyage AI / OpenAI (cloud) | Local default for zero-cost dev/test |
| Auth | Clerk or email/password JWT | Fast for solo dev |
| Deploy | Vercel + Railway **OR** VPS (Hetzner/DO) **OR** local-only initially | Decide based on when you want users. Cloud = zero ops. VPS = cheaper at scale. |
| Desktop | Tauri 2.0 (V2+) | <3MB binary vs 150MB Electron |

---

## Monorepo Structure

```
testing-ide/
  package.json
  pnpm-workspace.yaml
  turbo.json
  docker-compose.yml
  .env.example
  plan/                   -- Planning documents (this file lives here)
  packages/
    shared/               -- Shared TS types + Zod schemas
  apps/
    client/               -- React frontend (Vite)
      src/components/
      src/stores/
      src/hooks/
      src/pages/
      src/lib/
    server/               -- Express backend
      src/routes/
      src/services/
      src/workers/
      src/db/
      src/utils/
  tools/scripts/
```

---

## Frontend Layout

```
+----------------------------------------------------------+
| Toolbar: [Upload Project] [New Analysis] [Export] [Auth]  |
+------------+-------------------------+-------------------+
| File       | Editor Pane             | AI Panel          |
| Explorer   | (Monaco read-only src,  | - Generate        |
| - Tree     |  editable for .md)      | - Review Queue    |
| - Search   |                         | - Progress        |
+------------+-------------------------+-------------------+
| Status Bar: [Project Stats] [Analysis Progress]          |
+----------------------------------------------------------+
```

---

## Model Strategy (Provider-Agnostic + Local-First)

**Core principle**: Provider-agnostic from day one. User brings their own API key OR runs models locally via Ollama. Dev/testing uses Ollama (no API credits required).

### Provider Options (User Configurable)

User picks provider in Settings → AI Provider:

| Provider | What | Use Case |
|----------|------|----------|
| **OpenAI** | GPT-4o, GPT-4o-mini, o1, o3-mini | User has OpenAI key |
| **Anthropic** | Claude Opus/Sonnet/Haiku | User has Claude key |
| **OpenRouter** | Unified gateway → any model (Qwen, DeepSeek, Llama, Claude, GPT) | Best for users wanting choice/cheap routing |
| **Ollama Cloud** | Hosted Ollama models (no local install) | Wants open-source models, no GPU |
| **Ollama Local** | Runs on user's PC, zero API cost | Privacy, offline, dev/test, no API budget |

App ships with Ollama Local as **default** so anyone can use it without signing up for paid API.

### Top 3 Open-Source Models (Ranked for Local PC Use)

Models picked specifically for **consumer hardware compatibility** (most users have 8-16GB RAM, optional 8-12GB VRAM GPU). All available via Ollama with one command.

#### #1 Qwen2.5-Coder-7B (Alibaba) — Best balance of quality + accessibility
- **Ollama**: `ollama pull qwen2.5-coder:7b` (4.7GB download)
- **License**: Apache 2.0
- **Context**: 128K native
- **Hardware**: Runs on 8GB VRAM GPU OR 16GB RAM (CPU mode), Apple Silicon M1+
- **Quality**: Matches GPT-4-class on code benchmarks despite size
- **Strengths**: Code-specialized, native function calling, fast on consumer hardware
- **Use for**: Default local model — handles all artifact types acceptably
- **Larger variant**: `qwen2.5-coder:32b` for users with 24GB VRAM (RTX 4090) — much higher quality

#### #2 DeepSeek-R1-Distill-Qwen-7B (DeepSeek) — Best reasoning at small size
- **Ollama**: `ollama pull deepseek-r1:7b` (4.7GB)
- **License**: MIT (DeepSeek distilled into Qwen base)
- **Context**: 128K
- **Hardware**: Same as #1 — 8GB VRAM or 16GB RAM
- **Strengths**: Chain-of-thought reasoning baked in — ideal for defect analysis (traces code paths, finds race conditions, null safety issues); shows its reasoning steps
- **Use for**: Defect reports, bug analysis where deep reasoning matters
- **Larger variant**: `deepseek-r1:14b` or `deepseek-r1:32b` for better hardware

#### #3 Codestral 22B (Mistral) — Best for users with mid-range GPU
- **Ollama**: `ollama pull codestral:22b` (12GB)
- **License**: Mistral Non-Production License (free for personal use; commercial requires Mistral license — note for enterprise)
- **Context**: 32K (smaller — RAG matters more here)
- **Hardware**: 16GB+ VRAM (RTX 4080/4090) or 32GB+ unified memory (M2/M3 Max)
- **Strengths**: Pure code-specialization, very fast inference (FIM trained), excellent test scaffolding
- **Use for**: Test case bulk generation when user has decent hardware
- **Backup**: `llama3.1:8b` (Llama Community License, general-purpose) for users where Codestral license is a concern

### Hardware Tier → Model Recommendation

App detects user hardware on first run, recommends matching model:

| User Hardware | Recommended Model | Realistic Performance |
|---------------|-------------------|----------------------|
| 8GB RAM, no GPU (basic laptop) | qwen2.5-coder:7b (Q4 quant) | Slow (~5-10 tok/s CPU), works |
| 16GB RAM, no GPU | qwen2.5-coder:7b (Q4 or Q8) | Usable (~10-15 tok/s CPU) |
| 16GB RAM, 8GB VRAM (RTX 3060/4060) | qwen2.5-coder:7b OR deepseek-r1:7b | Fast (~40-60 tok/s) |
| 32GB RAM, 12-16GB VRAM (RTX 4070 Ti, M2 Pro) | qwen2.5-coder:14b | Fast, much better quality |
| 32GB+ RAM, 24GB VRAM (RTX 4090, M3 Max 64GB) | qwen2.5-coder:32b | Near GPT-4 quality, fast |
| Apple M-series 16GB+ unified | qwen2.5-coder:7b (MLX/Metal accelerated) | Excellent, low power |

For users who can't run any local model: Ollama Cloud or OpenRouter (cheap pay-per-use).

### Cloud/Server-Class Models (V1+ Pro Tier)

For users who want top quality and have API budget:

| Model | Provider | Use Case |
|-------|----------|----------|
| Claude Opus 4.x / Sonnet 4.x | Anthropic | Pro tier default — best structured output, prompt caching |
| GPT-4o / o1 / o3 | OpenAI | User preference |
| Qwen3-Coder-480B | OpenRouter / Together / Fireworks | Frontier open-source, code-tuned |
| DeepSeek-V3 / R1 (full) | OpenRouter / DeepSeek API | Frontier reasoning |
| Llama 4 Scout (10M context) | OpenRouter / Together | Whole-codebase ingestion, no chunking |

### Per-Task Model Routing

| Task | Local (Ollama) | Cloud Pro |
|------|----------------|-----------|
| Context.md generation | qwen2.5-coder:7b/32b | Claude Sonnet OR Llama 4 Scout |
| Test plan | qwen2.5-coder:7b | Claude Sonnet |
| Test cases (bulk) | qwen2.5-coder:7b | Qwen3-480B (OpenRouter, cheap) |
| Defect analysis | deepseek-r1:7b | Claude Opus / DeepSeek-R1 |
| Bug report | qwen2.5-coder:7b | Claude Haiku |

### Provider Configuration UI

```
Settings → AI Provider
  ┌─────────────────────────────────────────┐
  │  ○ OpenAI          [API Key Input]      │
  │  ○ Anthropic       [API Key Input]      │
  │  ○ OpenRouter      [API Key Input]      │
  │  ○ Ollama Cloud    [API Key Input]      │
  │  ● Ollama Local    [Detected ✓]         │
  │                                         │
  │  Model: [qwen2.5-coder:7b      ▼]      │
  │  [Test Connection]  [Save]              │
  └─────────────────────────────────────────┘
```

API keys stored encrypted in user's local DB (AES-256), never sent to any server except the chosen provider. For self-hosted deployments, keys can be set via env vars.

### Dev/Test Workflow (No API Credits Needed)

**Critical constraint**: Core AI agent functionality must be testable without paid APIs.

- **Local dev**: Spin up Ollama in Docker Compose, pull `qwen2.5-coder:7b`, run integration tests against it
- **CI**: Self-hosted runner with Ollama OR mock provider that replays cached responses
- **Coding agents (Claude Code, Cursor)**: OK to use during dev for writing code, but the AI agent's runtime tests use Ollama only
- **Smoke tests**: Use small fixed prompts → assert structural validity of output (JSON schema) rather than exact text

### LLMProvider Abstraction (Day 1)

Build with provider-agnostic interface so swapping providers is trivial:

```
interface LLMProvider {
  generate(messages, tools?, options?): Promise<Response>
  stream(messages, tools?, options?): AsyncIterable<Chunk>
  embed(texts): Promise<number[][]>  // for RAG
  countTokens(text): number
}

implementations:
  OpenAIProvider
  AnthropicProvider
  OpenRouterProvider     // OpenAI-compatible API
  OllamaProvider         // OpenAI-compatible API (local + cloud)
```

Ollama exposes OpenAI-compatible endpoint → reuse OpenAI SDK pointed at `http://localhost:11434/v1`. Reduces provider-specific code.

### Prompt Portability

- Use universal JSON Schema for function calling (works across all providers)
- Avoid Claude-specific XML tags in shared prompts; if needed, wrap in AnthropicProvider only
- Test each prompt against Ollama+Qwen (lowest common denominator) to ensure portability
- Smaller models need more explicit, structured prompts — design for them, larger models auto-adapt

---

## AI Pipeline (5 Stages — RAG in MVP)

1. **File Discovery** — walk dir, classify (source/config/test/docs), skip node_modules/vendor/binary
2. **AST Parsing** — Tree-sitter extracts functions, classes, imports, exports per file
3. **Semantic Chunking** — split at function/class boundaries, 500-1500 tokens per chunk
4. **Embedding + Indexing (RAG)** — Ollama `nomic-embed-text` (local, free) or Voyage AI/OpenAI (cloud) → pgvector HNSW index
5. **Hierarchical Summarization** — bottom-up LLM summarization → generates context.md

### Why RAG From Day 1 (Not V1+)

- **Local models** (Qwen2.5-Coder-7B): 128K context but slow on consumer hardware → RAG keeps prompts small → faster generation
- **Smaller local models** (e.g., Codestral 22B at 32K context): cannot fit medium projects → RAG mandatory
- **Cost reduction** for cloud users: smaller prompts = fewer tokens billed
- **Quality**: focused context produces better artifacts than dumping entire codebase
- **Scaling**: works for projects of any size from MVP

### RAG Flow

User requests "generate test cases for auth module":
1. Embed query → vector search top 20 relevant chunks
2. Walk dependency graph from those chunks → add called functions / imported modules
3. Add high-level context.md summary (always)
4. Pack into prompt within model's context budget
5. Stream LLM output

### Generation Flow
- User picks artifact type + scope (full project / module / file)
- System assembles: context.md + relevant code + dependency info
- Claude generates with structured output (tool_use schema)
- Stream result to frontend via SSE
- User reviews → approve / reject with feedback / regenerate

---

## Artifact Types

| Type | What It Generates |
|------|------------------|
| Test Plan | Scope, objectives, strategy, environments, risk matrix, entry/exit criteria |
| Test Cases | Individual cases with steps, expected results, priority, traceability to source |
| Defect Report | Static analysis findings with severity, category, location, suggested fix |
| Bug Report | Potential runtime issues formatted for tracking (steps to reproduce, root cause) |
| Test Summary | Executive-level coverage assessment, risk areas, recommendations |

MVP ships: Test Plan + Test Cases only. Others in V1.

---

## Database Schema (Core Tables)

- **users** — id, email, name, password_hash, plan, timestamps
- **user_provider_configs** — id, user_id, provider (openai/anthropic/openrouter/ollama-cloud/ollama-local), api_key_encrypted (AES-256), base_url, default_model, is_active
- **projects** — id, user_id, name, file_count, total_size, status, language_breakdown (JSONB)
- **project_files** — id, project_id, path, language, size, file_type, hash (SHA-256)
- **ast_analyses** — id, file_id, functions/classes/imports (JSONB)
- **code_chunks** — id, project_id, file_id, chunk_type, name, content, start_line, end_line, token_count, embedding BLOB (packed f32), embedding_dim, embedding_provider, embedding_model, metadata. Variable-length BLOB chosen over fixed-dim VECTOR so multiple embedding providers (nomic-embed-text 768, OpenAI 1536/3072, Voyage 1024) coexist; vector search filters by (project_id, embedding_provider, embedding_dim) before cosine to avoid comparing mismatched dimensions. See `apps/desktop/src-tauri/docs/adr/0001-blob-embeddings.md`.
- **artifacts** — id, project_id, type, title, content (MD), structured_data (JSONB), status, version
- **jobs** — id, project_id, type, status, progress, result (JSONB)

---

## Security (Critical Constraints)

- **NEVER execute uploaded code** — static analysis only, Tree-sitter parses as text
- File extension whitelist + magic bytes validation
- Size limits: 50MB/file, 500MB/project, 10K files max
- Secret scanning before LLM submission (redact API keys, passwords, tokens)
- JWT auth with 15-min access + 7-day refresh
- API keys server-side only, encrypted at rest (AES-256), never logged
- User-supplied API keys never sent anywhere except the user's chosen provider endpoint
- Path traversal prevention on file access
- HTTPS + CORS + Helmet security headers

---

## API Endpoints (Key)

- POST /api/projects — upload/create project
- GET /api/projects/:id/tree — file tree
- GET /api/projects/:id/files/:path — file content
- POST /api/projects/:id/analyze — trigger analysis
- POST /api/projects/:id/generate — generate artifact (type, scope, targets)
- GET /api/artifacts/:id — get artifact
- POST /api/artifacts/:id/approve — approve
- POST /api/artifacts/:id/regenerate — regenerate with feedback
- POST /api/artifacts/:id/export — export (markdown/json/pdf/jira)

---

## Output Formats

| Format | When | Implementation |
|--------|------|----------------|
| Markdown | Default, human review | Native |
| JSON | CI/CD integration | Custom schema |
| JIRA ADF | Enterprise ticket creation | Convert MD → Atlassian Document Format |
| PDF | Executive reports | Puppeteer headless rendering |

MVP: Markdown download only. Others in V1.

---

## Phased Roadmap

### Phase 1: MVP (Weeks 1-8)

**Week 1-2: Foundation**
- Monorepo setup (pnpm + Turborepo)
- React + Vite + Tailwind scaffold
- Express + TypeScript server
- PostgreSQL + pgvector schema (Drizzle)
- JWT auth
- Docker Compose for local dev (postgres + ollama services)
- Ollama auto-install detection / first-run wizard

**Week 3-4: Core UI**
- Three-panel layout
- File upload (folder picker)
- File tree (react-arborist)
- Monaco editor (read-only source)
- Markdown preview pane
- Tab system

**Week 5-6: AI Pipeline + RAG**
- Tree-sitter WASM (JS/TS/Python)
- File classification + AST extraction
- Semantic chunking (function/class boundaries)
- pgvector schema + HNSW index
- Embeddings via Ollama `nomic-embed-text` (default) — also support Voyage/OpenAI
- LLMProvider abstraction: OpenAI / Anthropic / OpenRouter / Ollama (Cloud + Local)
- Provider config UI (Settings → AI Provider) with API key input + connection test
- Hardware detection on first run → recommend matching local model
- Context.md generation (streaming, provider-agnostic)
- Test Plan + Test Cases generation
- SSE progress to frontend

**Week 7-8: Polish + Deploy**
- Review workflow (approve/reject/regenerate)
- Markdown export download
- Error handling + loading states
- Deploy (Railway + Vercel)
- Sentry monitoring
- Beta launch

### Phase 2: V1 (Weeks 9-16)
- Defect reports, bug reports, test summaries
- Multi-language (Java, Go, Rust, C#)
- PDF/JIRA/JSON export
- BullMQ job queue + Redis caching
- Multi-project support
- Billing (Stripe) for hosted Pro tier
- Per-task model routing (cheap model for bulk, smart model for analysis)
- Hierarchical summarization for very large repos
- Quality eval suite — golden tests across all supported providers/models

### Phase 3: V2 (Weeks 17-24)
- Team workspaces + collaboration
- Tauri desktop app
- Custom prompt templates
- **Offline mode** — Ollama running Qwen3-Coder-7B locally for desktop users (no API calls)
- SSO enterprise auth
- CI/CD REST API
- **Self-hosted enterprise** — vLLM/SGLang serving Qwen3-Coder-30B or DeepSeek-V3 on customer's infra (full data privacy)

---

## Cost Estimate

User brings own API key OR uses Ollama Local (free). App provider's costs are infra only — LLM costs pass to user via their own provider account.

### Dev/Test (Solo + small team, Ollama Local only)

| Service | Monthly |
|---------|---------|
| Local hardware (existing) | $0 |
| Ollama models (open-source) | $0 |
| Postgres (Docker local) | $0 |
| **Total dev cost** | **$0** — no API credits required |

### MVP Hosted (~100 users, BYO API key)

| Service | Monthly |
|---------|---------|
| Railway/VPS (API + DB) | $20-50 |
| Vercel (Frontend) | $0-20 |
| LLM API costs | **$0 to platform** — users pay their own provider |
| Sentry | $0 (free tier) |
| **Platform cost** | **~$20-70/month** |

### V1+ Optional Hosted Pro Tier

If platform offers managed Pro tier (no BYO key):
- Add $300-1500/month Claude or OpenRouter spend
- Pass through to users via Stripe subscription

### Self-Host Enterprise (V2+)

| Service | Monthly |
|---------|---------|
| GPU rental (1× H100 for qwen2.5-coder:32b or 30B-A3B) | $1100-2200 (24/7) or $200-500 (on-demand) |
| Or: customer's own GPU box | $0 incremental |
| Infra | $200-500 |

User-side cost (Ollama Local): $0 ongoing — uses their own hardware.

Cost controls: RAG (smaller prompts), model tiering per task, prompt caching when supported by provider, per-user limits on hosted tier.

---

## Risks + Mitigations

| Risk | Mitigation |
|------|-----------|
| LLM output inconsistency | Schema validation via tool_use, golden file tests, human review gate |
| API costs exceed budget | BYO key model — users pay their own provider. Default to Ollama Local (free). RAG keeps prompts small. |
| Vendor lock-in | LLMProvider abstraction from day 1. OpenAI / Anthropic / OpenRouter / Ollama all supported. Prompts test against Ollama (lowest common denominator). |
| Local model quality below cloud | Hardware tier detection recommends best model user can run. Larger users (32GB VRAM) get qwen2.5-coder:32b ≈ GPT-4-class. Pro tier optional for cloud. |
| User has no GPU / weak PC | qwen2.5-coder:7b runs on 16GB RAM CPU-only (slow but works). Ollama Cloud option if local impossible. |
| Dev/test needs API credits | Ollama runs in CI + local dev. All integration tests against Ollama. No cloud API required to develop or test the AI agent. |
| Codestral commercial license | Default recommendations are Apache-2.0 (Qwen) or MIT (DeepSeek-distilled). Codestral marked optional, requires user license check. |
| Large projects exceed context | RAG in V1, hierarchical summarization, scope-limited generation |
| Solo dev burnout | Ruthless MVP scoping, defer non-essentials, ship incrementally |
| LLM hallucinations | Confidence scores, source code references with line numbers, mandatory review |
| Security (uploaded code) | Never execute, whitelist extensions, secret scanning, sandboxed storage |

---

## Verification Plan

After each phase:
1. **Upload a real project** (e.g., a small Express API repo) into the IDE
2. Verify file tree renders correctly with proper file type icons
3. Click source files → verify Monaco displays with syntax highlighting
4. Trigger analysis → verify context.md generates with accurate project summary
5. Generate Test Plan → verify structured output with real test scenarios
6. Generate Test Cases → verify each case references actual functions/endpoints
7. Approve/reject artifacts → verify status persists
8. Export markdown → verify download contains correct content
9. Test with large project (1000+ files) → verify no crashes, reasonable performance
10. Security: upload .env file → verify blocked; upload binary → verify rejected

---

## Team Workflow (2-3 People)

**Suggested split:**
- **Person 1 (Frontend)**: React UI, Monaco integration, file tree, layout, state management
- **Person 2 (Backend + AI)**: API routes, Claude integration, AST pipeline, streaming
- **Person 3 (or shared)**: Database schema, auth, deployment, CI/CD, testing

**Interface contracts matter**: Define API types in `packages/shared/` first. Frontend and backend develop against shared types independently. Mock API responses for frontend dev while backend catches up.

**Parallel workstreams (Week 1-2)**:
- Frontend: layout + Monaco + file tree with mock data
- Backend: upload endpoint + Tree-sitter parsing + Claude streaming
- Shared: types, Docker Compose, DB schema

---

## First Day Actions

1. `pnpm init` + workspace config
2. Scaffold Vite React app + Express/FastAPI server
3. Docker Compose: postgres (pgvector) + ollama service
4. `docker exec ollama ollama pull qwen2.5-coder:7b` + `nomic-embed-text`
5. DB schema (users, projects, artifacts, code_chunks with vector column)
6. Define shared types in `packages/shared/`
7. LLMProvider interface + OllamaProvider (OpenAI-compatible endpoint at localhost:11434)
8. Monaco "hello world" in browser
9. Streaming "hello world" from server → Ollama → frontend (no API key needed)
10. Connect: button → API → streamed markdown in editor

Proves full stack end-to-end in ~2-3 days. **Zero API spend** for entire dev workflow. Team parallelizes from there.
