import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const desktopRoot = path.resolve(scriptDirectory, "..");
const playwrightCli = path.resolve(
  desktopRoot,
  "../../node_modules/@playwright/test/cli.js",
);

const child = spawn(
  process.execPath,
  [playwrightCli, "test", "-c", "e2e/playwright.config.ts"],
  {
    cwd: desktopRoot,
    env: {
      ...process.env,
      PLAYWRIGHT_BROWSERS_PATH: "0",
    },
    stdio: "inherit",
  },
);

child.on("exit", (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal);
    return;
  }

  process.exit(code ?? 1);
});
