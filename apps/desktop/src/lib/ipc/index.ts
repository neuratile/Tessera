/**
 * Typed wrappers around the Tauri IPC commands defined in
 * `apps/desktop/src-tauri/src/commands/`. Every wrapper validates the
 * response against a Zod schema from `@testing-ide/shared`, so UI code
 * always receives a typed value or an `IpcError`.
 *
 * Per `rules.md` §4.2.1: this is the only place in the frontend that
 * imports from `@tauri-apps/api/core`. Components consume these
 * functions, never `invoke` directly.
 */
export * as analysis from './analysis';
export * as artifacts from './artifacts';
export * as auth from './auth';
export * as filesystem from './filesystem';
export * as generation from './generation';
export * as hardware from './hardware';
export * as health from './health';
export * as ollama from './ollama';
export * as projects from './projects';
export * as providers from './providers';
export * as sandbox from './sandbox';
export * as streaming from './streaming';
export * as system from './system';

export { IpcError, getErrorMessage } from './error';
