import type { TrackerConfigView } from '@testing-ide/shared';
import { Check, Loader2, X, Trash2 } from 'lucide-react';
import { useCallback, useEffect, useState } from 'react';

import { Button } from '@/components/ui/button';
import { Input } from '@/components/ui/input';
import { trackers, getErrorMessage } from '@/lib/ipc';
import { toast } from '@/stores/toast-store';

const ISSUE_TYPE_OPTIONS = ['Task', 'Bug', 'Story', 'Epic'] as const;

export function JiraConfigPanel() {
  const [saved, setSaved] = useState<TrackerConfigView | null>(null);

  const [siteUrl, setSiteUrl] = useState('');
  const [email, setEmail] = useState('');
  const [apiToken, setApiToken] = useState('');
  const [projectKey, setProjectKey] = useState('');
  const [issueType, setIssueType] = useState('Task');
  const [isCustomIssueType, setIsCustomIssueType] = useState(false);

  const [error, setError] = useState<string | null>(null);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        const config = await trackers.getTrackerConfig('jira');
        if (config) {
          setSaved(config);
          setSiteUrl(config.siteUrl);
          setEmail(config.email);
          setProjectKey(config.projectKey);
          setIssueType(config.issueType);
          if (!ISSUE_TYPE_OPTIONS.includes(config.issueType as typeof ISSUE_TYPE_OPTIONS[number])) {
            setIsCustomIssueType(true);
          }
        }
      } catch (err) {
        setError(getErrorMessage(err));
      }
    })();
  }, []);

  const handleIssueTypeSelect = useCallback((val: string) => {
    if (val === '__custom__') {
      setIsCustomIssueType(true);
      setIssueType('');
    } else {
      setIsCustomIssueType(false);
      setIssueType(val);
    }
  }, []);

  const buildArgs = useCallback(
    () => ({
      tracker: 'jira',
      siteUrl: siteUrl.trim(),
      email: email.trim(),
      apiToken: apiToken.length > 0 ? apiToken : undefined,
      projectKey: projectKey.trim().toUpperCase(),
      issueType: issueType.trim(),
      isActive: true,
    }),
    [siteUrl, email, apiToken, projectKey, issueType],
  );

  const handleTest = useCallback(() => {
    setTesting(true);
    setTestResult(null);
    setTestError(null);
    void (async () => {
      try {
        const result = await trackers.testTrackerConnection({
          tracker: 'jira',
          siteUrl: siteUrl.trim(),
          email: email.trim(),
          apiToken: apiToken.length > 0 ? apiToken : undefined,
        });
        setTestResult(result);
      } catch (err) {
        setTestError(getErrorMessage(err));
      } finally {
        setTesting(false);
      }
    })();
  }, [siteUrl, email, apiToken]);

  const handleSave = useCallback(() => {
    setSaving(true);
    setError(null);
    void (async () => {
      try {
        await trackers.saveTrackerConfig(buildArgs());
        // The save command returns the row id; re-read the masked view so the
        // panel reflects the canonical persisted state (incl. hasApiToken).
        const view = await trackers.getTrackerConfig('jira');
        setSaved(view);
        setApiToken('');
        toast.ok('Jira integration settings saved successfully.', { title: 'Jira Integration' });
      } catch (err) {
        setError(getErrorMessage(err));
      } finally {
        setSaving(false);
      }
    })();
  }, [buildArgs]);

  const handleDelete = useCallback(() => {
    if (saved === null) return;
    const id = saved.id;
    setDeleting(true);
    setError(null);
    void (async () => {
      try {
        await trackers.deleteTrackerConfig(id);
        setSaved(null);
        setSiteUrl('');
        setEmail('');
        setApiToken('');
        setProjectKey('');
        setIssueType('Task');
        setIsCustomIssueType(false);
        setTestResult(null);
        setTestError(null);
        toast.ok('Jira integration settings deleted.', { title: 'Jira Integration' });
      } catch (err) {
        setError(getErrorMessage(err));
      } finally {
        setDeleting(false);
      }
    })();
  }, [saved]);

  const hasSavedToken = saved !== null && saved.hasApiToken;

  return (
    <section className="space-y-3 border-t border-border pt-4" data-testid="jira-config-panel">
      <div className="flex items-center justify-between">
        <h3 className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
          Jira Cloud Integration
        </h3>
        {saved && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            onClick={handleDelete}
            disabled={deleting}
            className="size-7 text-destructive hover:text-destructive hover:bg-destructive/10"
            title="Delete configuration"
          >
            {deleting ? <Loader2 className="size-3.5 animate-spin" /> : <Trash2 className="size-3.5" />}
          </Button>
        )}
      </div>
      <p className="text-muted-foreground text-[10px] leading-relaxed">
        Bridges Tessera's generated artifacts (Defect Report, Bug Report, Test Plan, etc.) into your Jira Cloud workspace.
      </p>

      <div className="space-y-3">
        <div className="space-y-1.5">
          <label htmlFor="jira-site-url" className="text-xs font-medium">
            Jira Site URL
          </label>
          <Input
            id="jira-site-url"
            value={siteUrl}
            onChange={(e) => setSiteUrl(e.target.value)}
            placeholder="https://acme.atlassian.net"
            autoComplete="off"
            spellCheck={false}
          />
        </div>

        <div className="space-y-1.5">
          <label htmlFor="jira-email" className="text-xs font-medium">
            Atlassian Email
          </label>
          <Input
            id="jira-email"
            type="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="user@example.com"
            autoComplete="off"
            spellCheck={false}
          />
        </div>

        <div className="space-y-1.5">
          <label htmlFor="jira-api-token" className="text-xs font-medium">
            API Token
          </label>
          <Input
            id="jira-api-token"
            type="password"
            value={apiToken}
            onChange={(e) => setApiToken(e.target.value)}
            placeholder={hasSavedToken ? 'Leave blank to keep the saved API token' : 'ATATT…'}
            autoComplete="off"
            spellCheck={false}
          />
          <p className="text-muted-foreground text-[10px]">
            Generate an API token in Atlassian Account Settings. Stored encrypted at rest (AES-GCM).
          </p>
        </div>

        <div className="grid grid-cols-2 gap-2">
          <div className="space-y-1.5">
            <label htmlFor="jira-project-key" className="text-xs font-medium">
              Default Project Key
            </label>
            <Input
              id="jira-project-key"
              value={projectKey}
              onChange={(e) => setProjectKey(e.target.value)}
              placeholder="PROJ"
              autoComplete="off"
              spellCheck={false}
            />
          </div>

          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label htmlFor="jira-issue-type" className="text-xs font-medium">
                Default Issue Type
              </label>
              <button
                type="button"
                onClick={() => {
                  setIsCustomIssueType(!isCustomIssueType);
                  setIssueType('Task');
                }}
                className="text-primary hover:underline text-[10px] font-medium"
              >
                {isCustomIssueType ? 'Use defaults' : 'Type custom...'}
              </button>
            </div>
            {isCustomIssueType ? (
              <Input
                id="jira-issue-type"
                value={issueType}
                onChange={(e) => setIssueType(e.target.value)}
                placeholder="Task"
                autoComplete="off"
                spellCheck={false}
              />
            ) : (
              <select
                id="jira-issue-type"
                value={issueType}
                onChange={(e) => handleIssueTypeSelect(e.target.value)}
                className="border-input bg-background text-foreground focus:ring-primary/40 focus:border-primary flex h-8 w-full rounded-md border px-2 py-1 text-xs transition-colors focus:outline-none focus:ring-2 disabled:cursor-not-allowed disabled:opacity-50"
              >
                {ISSUE_TYPE_OPTIONS.map((opt) => (
                  <option key={opt} value={opt}>
                    {opt}
                  </option>
                ))}
                <option value="__custom__">+ Custom issue type...</option>
              </select>
            )}
          </div>
        </div>
      </div>

      {testResult !== null ? (
        <div
          className="border-success/30 bg-success/5 text-success flex items-start gap-2 rounded-md border p-2 text-xs"
          role="status"
        >
          <Check className="mt-0.5 size-3.5 shrink-0" />
          <span>Connected as {testResult}</span>
        </div>
      ) : null}

      {testError !== null ? (
        <div
          className="border-destructive/30 bg-destructive/5 text-destructive flex items-start gap-2 rounded-md border p-2 text-xs"
          role="status"
        >
          <X className="mt-0.5 size-3.5 shrink-0" />
          <span className="min-w-0 flex-1">{testError}</span>
        </div>
      ) : null}

      {error !== null ? (
        <p className="text-destructive text-xs" role="alert">
          {error}
        </p>
      ) : null}

      <div className="flex items-center gap-2">
        <Button
          type="button"
          onClick={handleSave}
          disabled={saving || siteUrl.trim().length === 0 || email.trim().length === 0 || projectKey.trim().length === 0 || issueType.trim().length === 0}
        >
          {saving ? <Loader2 className="size-3.5 animate-spin" /> : null}
          Save Jira Config
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={handleTest}
          disabled={testing || siteUrl.trim().length === 0 || email.trim().length === 0}
        >
          {testing ? <Loader2 className="size-3.5 animate-spin" /> : null}
          Test Connection
        </Button>
      </div>
    </section>
  );
}
