/// <reference types="vite/client" />

declare module 'vite/client' {
  /**
   * Vite exposes only `import.meta.env` keys prefixed with `VITE_`.
   * See `apps/desktop/.env.example`.
   */
  // eslint-disable-next-line @typescript-eslint/consistent-type-definitions -- merges with Vite's base env typings
  interface ImportMetaEnv {
    readonly VITE_SENTRY_DSN?: string;
  }
}

declare const __APP_VERSION__: string;
