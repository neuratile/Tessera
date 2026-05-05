/**
 * localStorage key that records whether the first-run wizard has been
 * dismissed. Lives in its own module so the wizard component file only
 * exports React components (Vite fast-refresh requirement enforced by
 * `react-refresh/only-export-components`).
 */
export const ONBOARDING_STORAGE_KEY = 'testing-ide.onboarding.complete';

export function readOnboardingFlag(): boolean {
  try {
    return window.localStorage.getItem(ONBOARDING_STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

export function markOnboardingComplete(): void {
  try {
    window.localStorage.setItem(ONBOARDING_STORAGE_KEY, 'true');
  } catch {
    // localStorage may be unavailable under strict CSP — fail open so
    // the user is not blocked.
  }
}
