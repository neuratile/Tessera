import { describe, expect, it } from 'vitest';

import { ExportFormatSchema, ExportOutcomeSchema } from './export.schema';

describe('ExportFormatSchema', () => {
  it('accepts every Rust-side wire value', () => {
    expect(ExportFormatSchema.parse('xlsx')).toBe('xlsx');
    expect(ExportFormatSchema.parse('csv')).toBe('csv');
    expect(ExportFormatSchema.parse('tsv')).toBe('tsv');
  });

  it('rejects unknown formats and wrong casing', () => {
    expect(ExportFormatSchema.safeParse('pdf').success).toBe(false);
    expect(ExportFormatSchema.safeParse('XLSX').success).toBe(false);
    expect(ExportFormatSchema.safeParse('').success).toBe(false);
  });
});

describe('ExportOutcomeSchema', () => {
  it('round-trips a single-file outcome', () => {
    const outcome = { files: ['C:/exports/cases.xlsx'] };
    expect(ExportOutcomeSchema.parse(outcome)).toEqual(outcome);
  });

  it('round-trips a multi-file (CSV sibling) outcome', () => {
    const outcome = {
      files: ['C:/exports/cases.csv', 'C:/exports/cases.files.csv'],
    };
    expect(ExportOutcomeSchema.parse(outcome)).toEqual(outcome);
  });

  it('rejects an empty file list — the backend always writes at least one', () => {
    expect(ExportOutcomeSchema.safeParse({ files: [] }).success).toBe(false);
  });

  it('rejects missing or malformed files field', () => {
    expect(ExportOutcomeSchema.safeParse({}).success).toBe(false);
    expect(ExportOutcomeSchema.safeParse({ files: [''] }).success).toBe(false);
    expect(ExportOutcomeSchema.safeParse({ files: 'a.csv' }).success).toBe(false);
  });
});
