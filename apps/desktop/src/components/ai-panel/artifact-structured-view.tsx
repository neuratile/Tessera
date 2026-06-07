import {
  BugReportSchema,
  DefectReportSchema,
  TestCaseSchema,
  TestPlanSchema,
} from '@testing-ide/shared';
import type { BugReport, DefectReport, TestCase, TestPlan } from '@testing-ide/shared';

/**
 * Structured renderers for v2 artifact payloads (plan/ARTIFACT_QUALITY_V2.md
 * Phases 1 + 2): step tables for test cases, repro steps + severity/priority
 * split for bug reports, scope/criteria sections for test plans, and
 * CWE-categorized finding cards for defect reports.
 *
 * Parsing is best-effort: `structuredData` is `unknown` on the wire and
 * older v1 artifacts (plain-string steps, flat scopeIn/scopeOut, string
 * locations) do not match the v2 Zod schemas — for those
 * [`parseStructuredArtifact`] returns `null` and the drawer falls back to
 * the markdown body.
 */
export type ParsedStructuredArtifact =
  | { kind: 'test-cases'; data: TestCase }
  | { kind: 'bug-report'; data: BugReport }
  | { kind: 'test-plan'; data: TestPlan }
  | { kind: 'defect-report'; data: DefectReport };

export function parseStructuredArtifact(
  artifactType: string,
  structuredData: unknown,
): ParsedStructuredArtifact | null {
  if (artifactType === 'test-cases') {
    const parsed = TestCaseSchema.safeParse(structuredData);
    return parsed.success ? { kind: 'test-cases', data: parsed.data } : null;
  }
  if (artifactType === 'bug-report') {
    const parsed = BugReportSchema.safeParse(structuredData);
    return parsed.success ? { kind: 'bug-report', data: parsed.data } : null;
  }
  if (artifactType === 'test-plan') {
    const parsed = TestPlanSchema.safeParse(structuredData);
    return parsed.success ? { kind: 'test-plan', data: parsed.data } : null;
  }
  if (artifactType === 'defect-report') {
    const parsed = DefectReportSchema.safeParse(structuredData);
    return parsed.success ? { kind: 'defect-report', data: parsed.data } : null;
  }
  return null;
}

export function ArtifactStructuredView({ parsed }: { parsed: ParsedStructuredArtifact }) {
  switch (parsed.kind) {
    case 'test-cases':
      return <TestCasesView data={parsed.data} />;
    case 'bug-report':
      return <BugReportView data={parsed.data} />;
    case 'test-plan':
      return <TestPlanView data={parsed.data} />;
    case 'defect-report':
      return <DefectReportView data={parsed.data} />;
  }
}

const FIELD_LABEL_CLASS =
  'text-muted-foreground text-[10px] font-semibold uppercase tracking-[0.12em]';

function Pill({ label, value }: { label: string; value: string }) {
  return (
    <span className="bg-surface-2 text-muted-foreground inline-flex items-center gap-1 rounded border border-border px-1.5 py-0.5 font-mono text-[10px]">
      <span className="uppercase tracking-[0.08em]">{label}</span>
      <span className="text-foreground font-semibold">{value}</span>
    </span>
  );
}

/** Verbatim code-evidence block shared by bug and defect cards. */
function EvidenceBlock({ snippet }: { snippet: string | undefined }) {
  if (snippet === undefined || snippet.length === 0) return null;
  return (
    <div>
      <p className={FIELD_LABEL_CLASS}>Evidence</p>
      <pre className="bg-surface-2 mt-1 overflow-x-auto rounded border border-border p-2 font-mono text-[11px]">
        {snippet}
      </pre>
    </div>
  );
}

function StringList({ label, items }: { label: string; items: readonly string[] | undefined }) {
  if (items === undefined || items.length === 0) return null;
  return (
    <div>
      <p className={FIELD_LABEL_CLASS}>{label}</p>
      <ul className="mt-1 list-disc space-y-0.5 pl-5 text-xs">
        {/* Ordered, never re-ordered list; items can repeat, so the index is the stable key. */}
        {items.map((item, i) => (
          <li key={`${label}-${i}`}>{item}</li>
        ))}
      </ul>
    </div>
  );
}

function TestCasesView({ data }: { data: TestCase }) {
  return (
    <div className="space-y-4">
      {data.cases.map((tc) => (
        <article key={tc.id} className="rounded-md border border-border bg-card p-3 space-y-2">
          <header className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-xs font-semibold text-foreground">{tc.id}</span>
            <Pill label="type" value={tc.type} />
            <Pill label="priority" value={tc.priority} />
          </header>
          <h3 className="text-sm font-medium text-foreground">{tc.title}</h3>
          <StringList label="Preconditions" items={tc.preconditions} />
          {tc.testData !== undefined && tc.testData.length > 0 ? (
            <div>
              <p className={FIELD_LABEL_CLASS}>Test data</p>
              <p className="mt-1 font-mono text-xs">{tc.testData}</p>
            </div>
          ) : null}
          <div>
            <p className={FIELD_LABEL_CLASS}>Steps</p>
            <table className="mt-1 w-full border-collapse text-xs">
              <thead>
                <tr className="border-b border-border text-left">
                  <th className="text-muted-foreground w-8 py-1 pr-2 font-semibold">#</th>
                  <th className="text-muted-foreground py-1 pr-2 font-semibold">Action</th>
                  <th className="text-muted-foreground py-1 font-semibold">Expected result</th>
                </tr>
              </thead>
              <tbody>
                {tc.steps.map((step, i) => (
                  <tr key={`${tc.id}-step-${i}`} className="border-b border-border/50 align-top">
                    <td className="text-muted-foreground py-1 pr-2 font-mono">{i + 1}</td>
                    <td className="py-1 pr-2">{step.action}</td>
                    <td className="py-1">{step.expectedResult}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <StringList label="Postconditions" items={tc.postconditions} />
          <StringList label="Traceability" items={tc.traceability} />
        </article>
      ))}
    </div>
  );
}

function BugReportView({ data }: { data: BugReport }) {
  return (
    <div className="space-y-4">
      {data.bugs.map((bug) => (
        <article key={bug.id} className="rounded-md border border-border bg-card p-3 space-y-2">
          <header className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-xs font-semibold text-foreground">{bug.id}</span>
            <Pill label="severity" value={bug.severity} />
            <Pill label="priority" value={bug.priority} />
            <Pill label="repro" value={bug.reproducibility} />
          </header>
          <h3 className="text-sm font-medium text-foreground">{bug.title}</h3>
          {bug.environment !== undefined || bug.component !== undefined ? (
            <p className="text-muted-foreground font-mono text-[10px]">
              {[bug.component, bug.environment].filter((v) => v !== undefined).join(' · ')}
            </p>
          ) : null}
          <div>
            <p className={FIELD_LABEL_CLASS}>Steps to reproduce</p>
            <ol className="mt-1 list-decimal space-y-0.5 pl-5 text-xs">
              {bug.stepsToReproduce.map((step, i) => (
                <li key={`${bug.id}-repro-${i}`}>{step.replace(/^\d+[.)]\s*/, '')}</li>
              ))}
            </ol>
          </div>
          <div className="grid grid-cols-2 gap-2">
            <div>
              <p className={FIELD_LABEL_CLASS}>Expected</p>
              <p className="mt-1 text-xs">{bug.expectedBehavior}</p>
            </div>
            <div>
              <p className={FIELD_LABEL_CLASS}>Actual</p>
              <p className="mt-1 text-xs">{bug.actualBehavior}</p>
            </div>
          </div>
          {bug.workaround !== undefined && bug.workaround.length > 0 ? (
            <div>
              <p className={FIELD_LABEL_CLASS}>Workaround</p>
              <p className="mt-1 text-xs">{bug.workaround}</p>
            </div>
          ) : null}
          <div>
            <p className={FIELD_LABEL_CLASS}>Root cause</p>
            <p className="mt-1 font-mono text-xs">
              {bug.rootCause.symbol}
              {bug.rootCause.fileHint !== undefined ? ` — ${bug.rootCause.fileHint}` : ''}
              {bug.rootCause.startLine !== undefined && bug.rootCause.endLine !== undefined
                ? `:${bug.rootCause.startLine}–${bug.rootCause.endLine}`
                : ''}
            </p>
            <p className="mt-1 text-xs">{bug.rootCause.explanation}</p>
          </div>
          <EvidenceBlock snippet={bug.evidenceSnippet} />
        </article>
      ))}
    </div>
  );
}

function TestPlanView({ data }: { data: TestPlan }) {
  return (
    <div className="space-y-4">
      <p className="text-sm">{data.summary}</p>
      <div className="flex flex-wrap gap-1.5">
        {data.testLevels.map((level, i) => (
          <Pill key={`level-${i}`} label="level" value={level} />
        ))}
        {data.testTypes.map((type, i) => (
          <Pill key={`type-${i}`} label="type" value={type} />
        ))}
      </div>
      <StringList label="Objectives" items={data.objectives} />
      <div className="grid grid-cols-2 gap-2 rounded-md border border-border bg-card p-3">
        <StringList label="In scope" items={data.scope.inScope} />
        <StringList label="Out of scope" items={data.scope.outOfScope} />
      </div>
      <div>
        <p className={FIELD_LABEL_CLASS}>Strategy</p>
        <p className="mt-1 text-xs">{data.strategy}</p>
      </div>
      <StringList label="Environments" items={data.environments} />
      {data.risks.length > 0 ? (
        <div>
          <p className={FIELD_LABEL_CLASS}>Risks</p>
          <table className="mt-1 w-full border-collapse text-xs">
            <thead>
              <tr className="border-b border-border text-left">
                <th className="text-muted-foreground py-1 pr-2 font-semibold">Risk</th>
                <th className="text-muted-foreground py-1 font-semibold">Mitigation</th>
              </tr>
            </thead>
            <tbody>
              {data.risks.map((risk, i) => (
                <tr key={`risk-${i}`} className="border-b border-border/50 align-top">
                  <td className="py-1 pr-2">{risk.description}</td>
                  <td className="py-1">{risk.mitigation ?? '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : null}
      <div className="grid grid-cols-3 gap-2 rounded-md border border-border bg-card p-3">
        <StringList label="Entry criteria" items={data.entryCriteria} />
        <StringList label="Exit criteria" items={data.exitCriteria} />
        <StringList label="Suspension criteria" items={data.suspensionCriteria} />
      </div>
      <StringList label="Deliverables" items={data.deliverables} />
    </div>
  );
}

function DefectReportView({ data }: { data: DefectReport }) {
  return (
    <div className="space-y-4">
      {data.summary !== undefined && data.summary.length > 0 ? (
        <p className="text-sm">{data.summary}</p>
      ) : null}
      {data.findings.map((finding) => (
        <article key={finding.id} className="rounded-md border border-border bg-card p-3 space-y-2">
          <header className="flex flex-wrap items-center gap-2">
            <span className="font-mono text-xs font-semibold text-foreground">{finding.id}</span>
            <Pill label="severity" value={finding.severity} />
            <Pill label="category" value={finding.category} />
            <Pill label="confidence" value={finding.confidence} />
          </header>
          <p className="text-muted-foreground font-mono text-[10px]">
            {finding.location.symbol}
            {finding.location.fileHint !== undefined ? ` — ${finding.location.fileHint}` : ''}
            {`:${finding.location.startLine}–${finding.location.endLine}`}
          </p>
          <p className="text-xs">{finding.description}</p>
          <div>
            <p className={FIELD_LABEL_CLASS}>Impact</p>
            <p className="mt-1 text-xs">{finding.impact}</p>
          </div>
          <div>
            <p className={FIELD_LABEL_CLASS}>Suggested fix</p>
            <p className="mt-1 text-xs">{finding.fixSuggestion}</p>
          </div>
          <EvidenceBlock snippet={finding.evidenceSnippet} />
        </article>
      ))}
    </div>
  );
}
