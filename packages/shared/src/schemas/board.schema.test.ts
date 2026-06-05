import { describe, expect, it } from 'vitest';

import {
  ActivityLogSchema,
  BoardSchema,
  SprintSchema,
  TeamSchema,
} from './board.schema';

const UUID_A = '123e4567-e89b-12d3-a456-426614174000';
const UUID_B = '123e4567-e89b-12d3-a456-426614174001';
const ISO_TIMESTAMP = '2026-06-05T12:00:00.000Z';

describe('TeamSchema', () => {
  it('accepts a team without a description (nullable in DB)', () => {
    const parsed = TeamSchema.parse({
      id: UUID_A,
      name: 'core',
      inviteCode: 'ABC123',
      createdBy: UUID_B,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
    expect(parsed.description).toBeUndefined();
  });
});

describe('BoardSchema', () => {
  it('accepts a board without a description (nullable in DB)', () => {
    const parsed = BoardSchema.parse({
      id: UUID_A,
      teamId: UUID_B,
      name: 'Main',
      key: 'MAIN',
      boardType: 'kanban',
      issueCounter: 0,
      createdAt: ISO_TIMESTAMP,
      updatedAt: ISO_TIMESTAMP,
    });
    expect(parsed.description).toBeUndefined();
  });
});

describe('SprintSchema', () => {
  it('accepts a planned sprint with no goal or dates yet', () => {
    const parsed = SprintSchema.parse({
      id: UUID_A,
      boardId: UUID_B,
      name: 'Sprint 1',
      status: 'planned',
      createdAt: ISO_TIMESTAMP,
    });
    expect(parsed.goal).toBeUndefined();
    expect(parsed.startDate).toBeUndefined();
    expect(parsed.endDate).toBeUndefined();
  });

  it('accepts an active sprint with goal and dates', () => {
    const parsed = SprintSchema.parse({
      id: UUID_A,
      boardId: UUID_B,
      name: 'Sprint 2',
      goal: 'Ship boards',
      startDate: ISO_TIMESTAMP,
      endDate: ISO_TIMESTAMP,
      status: 'active',
      createdAt: ISO_TIMESTAMP,
    });
    expect(parsed.goal).toBe('Ship boards');
  });

  it('rejects an unknown sprint status', () => {
    expect(
      SprintSchema.safeParse({
        id: UUID_A,
        boardId: UUID_B,
        name: 'Sprint 3',
        status: 'archived',
        createdAt: ISO_TIMESTAMP,
      }).success,
    ).toBe(false);
  });
});

describe('ActivityLogSchema', () => {
  it('accepts a creation-style entry with no field/oldValue/newValue', () => {
    const parsed = ActivityLogSchema.parse({
      id: UUID_A,
      issueId: UUID_B,
      userId: UUID_A,
      action: 'created',
      createdAt: ISO_TIMESTAMP,
    });
    expect(parsed.field).toBeUndefined();
    expect(parsed.oldValue).toBeUndefined();
    expect(parsed.newValue).toBeUndefined();
  });

  it('accepts a field-change entry with old and new values', () => {
    const parsed = ActivityLogSchema.parse({
      id: UUID_A,
      issueId: UUID_B,
      userId: UUID_A,
      action: 'updated',
      field: 'priority',
      oldValue: 'low',
      newValue: 'high',
      createdAt: ISO_TIMESTAMP,
    });
    expect(parsed.field).toBe('priority');
  });
});
