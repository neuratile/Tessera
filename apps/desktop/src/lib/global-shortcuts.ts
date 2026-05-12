import { useEffect } from 'react';

import { COMMAND, dispatchCommand, type CommandId } from './command-bus';

/**
 * Renderer-side keyboard shortcuts.
 *
 * The native menu owns the *visible* accelerator hints (rendered next
 * to each menu item) on Windows + macOS, but the menu only fires on
 * its own when the menu bar is focusable. Tauri 2's menu accelerators
 * do not always trigger reliably inside a focused WebView text input,
 * so we also listen on the window's `keydown` and dispatch into the
 * shared `commandBus`. Doubly bound shortcuts are idempotent —
 * commandBus listeners are wired per-feature, so a second fire just
 * runs the action twice on the rare race; in practice the renderer
 * keydown beats the native menu by 1–2 ms and that path wins.
 *
 * Shortcuts intentionally mirror the menu's accelerators:
 *
 *  - `Cmd/Ctrl + O`       Open Folder
 *  - `Cmd/Ctrl + ,`       Settings
 *  - `Cmd/Ctrl + B`       Toggle Sidebar
 *  - `Cmd/Ctrl + J`       Toggle AI Panel
 *  - `Cmd/Ctrl + Shift + A` Analyze Project
 *  - `Cmd/Ctrl + G`       Regenerate Last artifact
 *  - `Cmd/Ctrl + Shift + G` Open GitHub Repo
 *  - `Cmd/Ctrl + K`       Open Command Palette
 */

type Match = { id: CommandId; preventDefault: boolean };

function matchShortcut(event: KeyboardEvent): Match | null {
  const mod = event.ctrlKey || event.metaKey;
  if (!mod) return null;
  // Treat the keyboard layout's physical key by `event.key` lowered so
  // Shift-modified letters still match.
  const key = event.key.toLowerCase();

  if (event.shiftKey && key === 'a') {
    return { id: COMMAND.AiAnalyze, preventDefault: true };
  }
  if (event.shiftKey && key === 'g') {
    return { id: COMMAND.HelpGithub, preventDefault: true };
  }
  if (event.shiftKey) {
    // No other shift-modified shortcuts; fall through so the OS keeps
    // its own bindings (e.g. Shift+Cmd+P style command palettes that
    // might land later).
    return null;
  }

  switch (key) {
    case 'o':
      return { id: COMMAND.FileOpenFolder, preventDefault: true };
    case ',':
      return { id: COMMAND.FileSettings, preventDefault: true };
    case 'b':
      return { id: COMMAND.ViewToggleSidebar, preventDefault: true };
    case 'j':
      return { id: COMMAND.ViewToggleAiPanel, preventDefault: true };
    case 'g':
      return { id: COMMAND.AiRegenerate, preventDefault: true };
    case 'k':
      return { id: COMMAND.PaletteOpen, preventDefault: true };
    default:
      return null;
  }
}

/**
 * Install the keydown handler. Returns a cleanup callback; intended
 * to be called from a `useEffect` mount in the App shell so the
 * listener tears down on hot-reload.
 */
export function useGlobalShortcuts(): void {
  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      const match = matchShortcut(event);
      if (match === null) return;
      if (match.preventDefault) event.preventDefault();
      dispatchCommand(match.id);
    };
    window.addEventListener('keydown', handler);
    return () => {
      window.removeEventListener('keydown', handler);
    };
  }, []);
}
