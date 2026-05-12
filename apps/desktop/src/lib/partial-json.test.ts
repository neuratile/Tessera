import { describe, expect, it } from 'vitest';

import { extractStreamingPreview } from './partial-json';

describe('extractStreamingPreview', () => {
  it('returns null for an empty buffer', () => {
    expect(extractStreamingPreview('')).toBeNull();
  });

  it('returns null when no whitelisted key is present', () => {
    expect(extractStreamingPreview('{"id":"123","kind":"abc"}')).toBeNull();
  });

  it('extracts a completed summary value', () => {
    expect(
      extractStreamingPreview('{"summary":"Hello world","other":"x"}'),
    ).toBe('Hello world');
  });

  it('extracts an unterminated tail string mid-stream', () => {
    const buf = '{"summary":"The auth module verifies JWTs and ';
    expect(extractStreamingPreview(buf)).toBe(
      'The auth module verifies JWTs and ',
    );
  });

  it('prefers the latest preview field in the buffer', () => {
    // `title` lands before a later `description`. We want the
    // latest because that is what the model is actively writing.
    const buf = '{"title":"old","description":"newer text"}';
    expect(extractStreamingPreview(buf)).toBe('newer text');
  });

  it('decodes escape sequences in completed values', () => {
    expect(
      extractStreamingPreview('{"summary":"line1\\nline2\\ttab"}'),
    ).toBe('line1\nline2\ttab');
  });

  it('handles arrays of objects — finds the deepest preview key', () => {
    const buf = '{"cases":[{"id":"x","title":"first"},{"id":"y","title":"second';
    expect(extractStreamingPreview(buf)).toBe('second');
  });

  it('ignores values that are not associated with whitelisted keys', () => {
    expect(
      extractStreamingPreview('{"unrelated":"ignored value"}'),
    ).toBeNull();
  });
});
