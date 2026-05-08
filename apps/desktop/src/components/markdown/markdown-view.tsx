import type { Components } from 'react-markdown';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

/**
 * XSS-safe markdown viewer.
 *
 * Security model:
 * - `react-markdown`'s default behaviour is to ignore raw HTML. We
 *   deliberately do NOT add `rehype-raw` — LLM output is untrusted and
 *   raw `<script>` / `<iframe>` / `onclick=` payloads must not reach
 *   the DOM.
 * - `remark-gfm` adds tables / strikethrough / task lists (Markdown
 *   features the prompts emit) without enabling raw HTML.
 * - Link rendering is overridden so anchors open in a new browsing
 *   context (`target="_blank"`) with `rel="noreferrer noopener"` to
 *   avoid `window.opener` leaks. We also strip `javascript:` URLs.
 * - Code blocks render to a plain `<pre><code>` — no syntax
 *   highlighting plugin (would re-introduce a parser surface and
 *   doesn't render anything user-controlled as HTML).
 */
export function MarkdownView({ source }: { source: string }) {
  // Tessera markdown chrome — `prose` typography overridden by hand
  // so the rendered artifact lines up with the IDE's monospaced /
  // teal-accent aesthetic rather than the Tailwind Typography slate
  // defaults. Headings drop margin so multi-section artifacts stay
  // readable in a 540 px drawer.
  return (
    <div
      className="prose prose-sm dark:prose-invert max-w-none break-words
        prose-headings:tracking-tight prose-headings:text-foreground
        prose-h1:text-base prose-h2:text-sm prose-h3:text-[13px]
        prose-p:text-xs prose-p:leading-relaxed prose-p:text-foreground
        prose-li:text-xs prose-li:text-foreground
        prose-strong:text-foreground
        prose-a:text-primary prose-a:no-underline hover:prose-a:underline
        prose-hr:border-border"
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={MARKDOWN_COMPONENTS}>
        {source}
      </ReactMarkdown>
    </div>
  );
}

const MARKDOWN_COMPONENTS: Components = {
  a({ href, children, ...rest }) {
    const safe = isSafeHref(href);
    return (
      <a
        {...rest}
        href={safe ? href : undefined}
        target="_blank"
        rel="noreferrer noopener"
        className="text-primary underline underline-offset-2 hover:text-primary/80"
      >
        {children}
      </a>
    );
  },
  code({ className, children }) {
    const isBlock = typeof className === 'string' && className.startsWith('language-');
    if (isBlock) {
      return (
        <pre className="bg-surface-2 text-foreground overflow-x-auto rounded-md border border-border p-3 font-mono text-[11px] leading-relaxed">
          <code className={className}>{children}</code>
        </pre>
      );
    }
    return (
      <code className="bg-surface-3 text-foreground rounded px-1 py-0.5 font-mono text-[11px]">
        {children}
      </code>
    );
  },
  table({ children }) {
    return (
      <div className="border-border my-3 overflow-x-auto rounded-md border">
        <table className="w-full border-collapse text-xs">{children}</table>
      </div>
    );
  },
  th({ children }) {
    return (
      <th className="border-border bg-surface-3 text-foreground border-b px-2 py-1.5 text-left text-[10px] font-semibold uppercase tracking-[0.08em]">
        {children}
      </th>
    );
  },
  td({ children }) {
    return (
      <td className="border-border text-foreground border-t px-2 py-1.5 align-top">
        {children}
      </td>
    );
  },
};

/**
 * Allow only `http(s)` and `mailto:` schemes. `react-markdown` already
 * filters `javascript:`, but we belt-and-brace the check so a future
 * upstream relaxation does not regress us.
 */
function isSafeHref(href: string | undefined): boolean {
  if (typeof href !== 'string' || href.length === 0) return false;
  const lower = href.trim().toLowerCase();
  return (
    lower.startsWith('http://') ||
    lower.startsWith('https://') ||
    lower.startsWith('mailto:') ||
    lower.startsWith('#') ||
    lower.startsWith('/')
  );
}
