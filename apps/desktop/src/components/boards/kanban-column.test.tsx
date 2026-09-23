// @vitest-environment jsdom
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { KanbanColumn } from './kanban-column';
import type { BoardColumn, Issue } from '@/stores/board-store';

afterEach(cleanup);

describe('KanbanColumn keyboard-accessible ordering', () => {
  it('moves an issue one place up or down through the existing drop handler', () => {
    const onDrop = vi.fn();
    const column = { id: 'column-1', name: 'To Do', color: '#fff' } as BoardColumn;
    const issues = [
      { id: 'one', issueKey: 'T-1', title: 'First', position: 0, issueType: 'task', priority: 'medium', labels: [] },
      { id: 'two', issueKey: 'T-2', title: 'Second', position: 1, issueType: 'task', priority: 'medium', labels: [] },
      { id: 'three', issueKey: 'T-3', title: 'Third', position: 2, issueType: 'task', priority: 'medium', labels: [] },
    ] as unknown as Issue[];
    render(<KanbanColumn column={column} issues={issues} onDrop={onDrop} onIssueClick={vi.fn()} onCreateIssue={vi.fn()} />);

    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Move T-1 up in To Do' }).disabled).toBe(true);
    expect(screen.getByRole<HTMLButtonElement>('button', { name: 'Move T-3 down in To Do' }).disabled).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Move T-2 up in To Do' }));
    fireEvent.click(screen.getByRole('button', { name: 'Move T-2 down in To Do' }));
    expect(onDrop).toHaveBeenNthCalledWith(1, 'two', 'column-1', 0);
    expect(onDrop).toHaveBeenNthCalledWith(2, 'two', 'column-1', 2);
  });
});
