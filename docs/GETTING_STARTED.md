# Getting started

## Requirements and launch

Use Git, pnpm 10.9.0, Node 20 or newer, stable Rust, and your platform's
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).
CI uses Node 20 and stable Rust on Linux. Docker is optional for generation
and required for sandbox execution.

```bash
git clone https://github.com/neuratile/Tessera.git tessera
cd tessera
corepack enable
pnpm install --frozen-lockfile
cp apps/desktop/.env.example apps/desktop/.env
pnpm --filter @testing-ide/desktop dev
```

Start with `apps/desktop/.env`; root `.env` is also used for Docker Compose.
Keep both ignored. Never put backend secrets in `VITE_*` variables, which
are bundled into the renderer. Example JWT fallback values are for throwaway
local development; use your own secret for deployments or shared setups.
The optional Boards API requires separate database/server configuration and
is not needed for local artifact generation.

### Windows shell troubleshooting

Install the MSVC Rust toolchain, Visual Studio C++ build tools, WebView2, and
Git Bash. If pnpm prints a `cmd.exe` banner instead of executing its script,
choose Git Bash for the current PowerShell session:

```powershell
$env:npm_config_script_shell = 'C:/Program Files/Git/bin/bash.exe'
pnpm install --frozen-lockfile
pnpm typecheck
```

Adjust the path to your Git installation. Turbo passes this setting through
while retaining its normal environment filtering. Git Bash and Cargo must also
be on PATH for the local pre-push guard.

## Configure Ollama

Start Ollama, then download a chat model and an embedding model:

```bash
ollama pull qwen2.5-coder:1.5b
ollama pull nomic-embed-text
```

These small models are used by live CI and are not bundled app assets.
Select a downloaded chat model and explicitly activate its connection in Settings.
Choose embeddings separately. Changing embedding provider/model can require
reindexing because provider and vector dimensions must remain compatible.

Alternatively, copy root `.env.example` to `.env` and use `pnpm services:up`
for Docker Compose services. The optional `pnpm bootstrap:ollama` helper requires
Node 22.6+ for TypeScript stripping; the manual commands avoid that requirement.

## Generate an artifact

1. Launch Tessera and open/import a small local project.
2. Finish provider setup and choose the active LLM connection.
3. Let indexing finish, then generate Context and a Test Plan or Test Cases.
4. Inspect generated output and relevant code before relying on it.
5. Export the artifact, or explicitly enable the sandbox to execute supported tests.

Local model and embedding providers keep generation context on your machine.
Cloud selections send relevant content to the selected provider.

## Try the review fixture

The staged-review runtime is still planned. Run its fixture now:

```bash
node --test evals/fixtures/checkout/fixture.test.mjs
```

The wrapper succeeds when the seeded regression fails as expected and baseline
and clean controls pass. See [evals](../evals/README.md) for temporary Git setup.
There is no working “Review my changes” button, CLI, or MCP server yet.

For contribution checks, see [CI/CD](./CI_CD.md).
