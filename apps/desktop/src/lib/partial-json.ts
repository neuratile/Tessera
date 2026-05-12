/**
 * Extract a human-readable preview from a partial JSON tool-call
 * buffer.
 *
 * Backend streams a serialized JSON object as it lands token-by-token
 * from the LLM. By the end the buffer is a valid object, but the
 * raw tail looks like
 *
 *   `{"summary":"The auth module verifies JWTs and...`
 *
 * — i.e. broken JSON, often with an unterminated trailing string.
 * Rendering that raw at the user is mostly noise. This helper walks
 * the buffer with a tiny one-pass state machine and returns the
 * most-recently-seen value of a whitelisted "preview" key
 * (`summary`, `description`, `title`, `prose`, `content`, `notes`,
 * `markdown`, `body`, `details`). When the value's closing `"` has
 * already streamed in we return the completed string; when it has
 * not, we return everything emitted so far (a live "typing" preview).
 *
 * The walker is intentionally permissive — it never throws and
 * never tries to validate the surrounding JSON structure. Bad input
 * just yields `null` and the caller falls back to the raw buffer.
 */

const PREVIEW_KEYS = new Set([
  'summary',
  'description',
  'title',
  'prose',
  'content',
  'notes',
  'markdown',
  'body',
  'details',
  'rationale',
  'expectedResult',
  'expected_result',
]);

type PreviewMatch = {
  /** Closing-quote index (exclusive) or `buf.length` if unterminated. */
  end: number;
  value: string;
};

export function extractStreamingPreview(buf: string): string | null {
  let best: PreviewMatch | null = null;

  const len = buf.length;
  let i = 0;
  while (i < len) {
    // Skip ahead until the next `"` that begins a string token.
    while (i < len && buf.charCodeAt(i) !== 34 /* " */) {
      i += 1;
    }
    if (i >= len) break;

    // Read a JSON string starting at `i` and produce its decoded
    // value + the index after the closing quote (or `len` if
    // unterminated). `terminated` tells us whether the closing
    // quote was seen — used to pick between completed vs live tail.
    const stringStart = i + 1;
    let j = stringStart;
    let value = '';
    let terminated = false;
    while (j < len) {
      const ch = buf.charCodeAt(j);
      if (ch === 92 /* \ */) {
        if (j + 1 >= len) {
          j = len; // ends mid-escape — treat as unterminated
          break;
        }
        const next = buf[j + 1];
        if (next === 'n') value += '\n';
        else if (next === 't') value += '\t';
        else if (next === 'r') value += '\r';
        else if (next === '"') value += '"';
        else if (next === '\\') value += '\\';
        else if (next === '/') value += '/';
        else if (next === 'u' && j + 5 < len) {
          const hex = buf.substring(j + 2, j + 6);
          const code = parseInt(hex, 16);
          if (Number.isFinite(code)) {
            value += String.fromCharCode(code);
          }
          j += 6;
          continue;
        } else if (next !== undefined) {
          value += next;
        }
        j += 2;
        continue;
      }
      if (ch === 34 /* " */) {
        terminated = true;
        j += 1;
        break;
      }
      value += buf[j];
      j += 1;
    }

    // We have a string token spanning `i .. j` with decoded `value`.
    // Decide whether this string is a *value* for a whitelisted
    // preview key. Look back from `i` past whitespace and a colon to
    // find another string token that names a key.
    let k = i - 1;
    while (k >= 0 && /\s/.test(buf[k] ?? '')) k -= 1;
    if (buf[k] === ':') {
      k -= 1;
      while (k >= 0 && /\s/.test(buf[k] ?? '')) k -= 1;
      if (buf[k] === '"') {
        // Walk back to the opening quote of the key string.
        const keyEnd = k;
        let keyStart = k - 1;
        while (keyStart >= 0) {
          if (buf[keyStart] === '"' && buf[keyStart - 1] !== '\\') break;
          keyStart -= 1;
        }
        if (keyStart >= 0 && keyEnd > keyStart) {
          const key = buf.substring(keyStart + 1, keyEnd).toLowerCase();
          if (PREVIEW_KEYS.has(key) && value.length > 0) {
            // Prefer the latest match across the buffer so the user
            // sees the most recent piece the model is writing.
            if (best === null || j >= best.end) {
              best = { end: j, value };
            }
            if (!terminated) {
              // The walker has consumed the rest of the buffer
              // chasing this unterminated string — no more keys
              // can follow, so exit the loop early.
              return best.value;
            }
          }
        }
      }
    }
    i = j > i ? j : i + 1;
  }

  return best?.value ?? null;
}
