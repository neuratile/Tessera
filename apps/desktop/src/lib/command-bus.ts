/**
 * Lightweight singleton command bus.
 *
 * Surfaces a single `EventTarget` that the rest of the renderer
 * publishes to + subscribes to. The native menu listener and the
 * global keyboard-shortcut hook both dispatch into the same bus, and
 * feature components (Toolbar, AI panel, Settings sheet) subscribe to
 * just the commands they own.
 *
 * Why not a Zustand action? Some commands ("open folder", "analyze
 * current project") live inside Toolbar's local component state and
 * are not trivially liftable into a global store without duplicating
 * the IPC orchestration. The bus lets us route a menu click straight
 * into the same handler the toolbar already exposes — no fan-out, no
 * prop-drilling, no extra store.
 */

import { useEffect } from 'react';

/**
 * Stable command ids. Mirrors `apps/desktop/src-tauri/src/menu.rs`'s
 * `ids` module so the renderer can match on the menu event payload
 * without redefining the names.
 */
export const COMMAND = {
  FileOpenFolder: 'file/open-folder',
  FileSettings: 'file/settings',
  ViewToggleSidebar: 'view/toggle-sidebar',
  ViewToggleAiPanel: 'view/toggle-ai-panel',
  AiAnalyze: 'ai/analyze',
  AiRegenerate: 'ai/regenerate',
  HelpDocs: 'help/docs',
  HelpGithub: 'help/github',
  /**
   * Open the renderer-side command palette (`Cmd/Ctrl+K`). Not
   * routed through the native menu — Tauri's menu API has no
   * predefined "command palette" item and we want the shortcut
   * captured even when the menu bar lost focus.
   */
  PaletteOpen: 'palette/open',
} as const;

export type CommandId = (typeof COMMAND)[keyof typeof COMMAND];

const ALL_COMMAND_IDS: ReadonlySet<string> = new Set(Object.values(COMMAND));

const target = new EventTarget();

/**
 * Fire a command. No-op if no listener is registered — components
 * that own a particular command subscribe in `useEffect`, and
 * commands dispatched before mount are deliberately dropped so a
 * race between menu init and renderer mount cannot replay.
 */
export function dispatchCommand(id: CommandId): void {
  target.dispatchEvent(new CustomEvent('command', { detail: id }));
}

/**
 * Type guard — narrows an unknown string to a valid `CommandId`
 * before dispatch. Used by the native-menu listener which receives
 * the id as a raw Tauri event payload.
 */
export function isCommandId(value: unknown): value is CommandId {
  return typeof value === 'string' && ALL_COMMAND_IDS.has(value);
}

/**
 * Subscribe to one command id. The handler runs every time that
 * command is dispatched until the returned `cleanup` is called.
 *
 * Intended for use inside `useEffect` in the component that owns
 * the command's side-effect.
 */
export function onCommand(id: CommandId, handler: () => void): () => void {
  const listener = (event: Event) => {
    if ((event as CustomEvent<string>).detail === id) {
      handler();
    }
  };
  target.addEventListener('command', listener);
  return () => target.removeEventListener('command', listener);
}

/**
 * React-friendly subscription wrapper. Re-binds when `handler`
 * identity changes so callers can wrap with `useCallback` to control
 * the dependency surface.
 */
export function useCommand(id: CommandId, handler: () => void): void {
  useEffect(() => onCommand(id, handler), [id, handler]);
}
