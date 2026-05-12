import { useId } from 'react';

/**
 * Stable, dialog-scoped id for the heading. Pair with
 * `<Dialog labelledBy={id}>` (from `@/components/ui/dialog`) so screen
 * readers announce the dialog title when focus moves into the
 * surface.
 *
 * Lives in `src/lib/` instead of next to `Dialog` because the
 * react-refresh plugin requires component files to export only
 * components — co-locating a hook alongside the component breaks
 * HMR boundaries.
 */
export function useDialogTitleId(): string {
  return useId();
}
