import { ChevronDown, ChevronUp, Plus } from 'lucide-react';
import { useCallback, useRef, useState } from 'react';

import { IssueCard } from '@/components/boards/issue-card';
import { useBoardStore, type BoardColumn as ColumnType, type Issue } from '@/stores/board-store';

type Props = {
  column: ColumnType;
  issues: Issue[];
  onIssueClick: (issueId: string) => void;
  onCreateIssue: (columnId: string) => void;
  onDrop: (issueId: string, columnId: string, position: number) => void;
};

export function KanbanColumn({
  column,
  issues,
  onIssueClick,
  onCreateIssue,
  onDrop,
}: Props) {
  const [dragOver, setDragOver] = useState(false);
  const [hoveredCardId, setHoveredCardId] = useState<string | null>(null);
  const [hoveredCardSide, setHoveredCardSide] = useState<'top' | 'bottom' | null>(null);
  const dragCounter = useRef(0);

  const draggedIssueId = useBoardStore((s) => s.draggedIssueId);
  const setDraggedIssueId = useBoardStore((s) => s.setDraggedIssueId);
  const dropRef = useRef<HTMLDivElement>(null);

  const sortedIssues = [...issues].sort((a, b) => a.position - b.position);
  const isOverWip =
    column.wipLimit !== undefined &&
    column.wipLimit !== null &&
    sortedIssues.length >= column.wipLimit;

  const handleDragStart = useCallback(
    (e: React.DragEvent, issueId: string) => {
      e.dataTransfer.setData('text/plain', issueId);
      e.dataTransfer.effectAllowed = 'move';
      
      // Use setTimeout to defer state setting after drag is fully initiated
      setTimeout(() => {
        setDraggedIssueId(issueId);
      }, 0);
    },
    [setDraggedIssueId],
  );

  const handleDragEnd = useCallback(() => {
    dragCounter.current = 0;
    setDraggedIssueId(null);
    setHoveredCardId(null);
    setHoveredCardSide(null);
    setDragOver(false);
  }, [setDraggedIssueId]);

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    dragCounter.current++;
    setDragOver(true);
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    setDragOver(true);
    // When dragging over column empty space, clear card hover indicators
    setHoveredCardId(null);
    setHoveredCardSide(null);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    dragCounter.current--;
    if (dragCounter.current <= 0) {
      dragCounter.current = 0;
      setDragOver(false);
      setHoveredCardId(null);
      setHoveredCardSide(null);
    }
  }, []);

  const handleDragOverCard = useCallback((e: React.DragEvent, cardId: string) => {
    e.preventDefault();
    e.stopPropagation();
    e.dataTransfer.dropEffect = 'move';
    
    if (draggedIssueId === cardId) return;

    const rect = e.currentTarget.getBoundingClientRect();
    const relativeY = e.clientY - rect.top;
    const side = relativeY < rect.height / 2 ? 'top' : 'bottom';
    
    setHoveredCardId((prev) => (prev !== cardId ? cardId : prev));
    setHoveredCardSide((prev) => (prev !== side ? side : prev));
    setDragOver(true);
  }, [draggedIssueId]);

  const handleDragLeaveCard = useCallback((cardId: string) => {
    // Only clear if the card we are leaving is the one currently marked as hovered
    setHoveredCardId((prev) => (prev === cardId ? null : prev));
    setHoveredCardSide((prev) => (prev === cardId ? null : prev));
  }, []);

  const handleDropOnCard = useCallback(
    (e: React.DragEvent, targetIssueId: string, targetPosition: number) => {
      e.preventDefault();
      e.stopPropagation();
      
      dragCounter.current = 0;
      setDragOver(false);
      setHoveredCardId(null);
      setHoveredCardSide(null);
      setDraggedIssueId(null);
      
      const issueId = draggedIssueId || e.dataTransfer.getData('text/plain');
      if (issueId && issueId !== targetIssueId) {
        const rect = e.currentTarget.getBoundingClientRect();
        const relativeY = e.clientY - rect.top;
        const isBefore = relativeY < rect.height / 2;
        
        const newPos = isBefore ? targetPosition : targetPosition + 1;
        onDrop(issueId, column.id, newPos);
      }
    },
    [column.id, onDrop, draggedIssueId, setDraggedIssueId],
  );

  const handleDropColumn = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      dragCounter.current = 0;
      setDragOver(false);
      setHoveredCardId(null);
      setHoveredCardSide(null);
      setDraggedIssueId(null);
      
      const issueId = draggedIssueId || e.dataTransfer.getData('text/plain');
      if (issueId) {
        // Drop at the bottom of the column
        const maxPosition = sortedIssues.reduce(
          (max, i) => Math.max(max, i.position),
          0,
        );
        onDrop(issueId, column.id, maxPosition + 1);
      }
    },
    [column.id, onDrop, sortedIssues, draggedIssueId, setDraggedIssueId],
  );

  return (
    <div
      ref={dropRef}
      className={`flex h-full w-full min-w-[200px] max-w-[360px] flex-1 flex-col rounded-xl border transition-colors duration-150 ${
        dragOver
          ? 'border-primary/40 bg-primary/5'
          : 'border-border/50 bg-surface-2/50'
      }`}
      onDragEnter={handleDragEnter}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDropColumn}
    >
      {/* Column header */}
      <div className="flex items-center justify-between px-3 py-2.5">
        <div className="flex items-center gap-2">
          <div
            className="size-2.5 rounded-full"
            style={{ backgroundColor: column.color }}
            aria-hidden="true"
          />
          <h3 className="text-sm font-semibold text-foreground">{column.name}</h3>
          <span className="flex size-5 items-center justify-center rounded-full bg-muted text-[10px] font-bold text-muted-foreground">
            {sortedIssues.length}
          </span>
          {column.wipLimit ? (
            <span
              className={`text-[10px] ${
                isOverWip ? 'font-bold text-destructive' : 'text-muted-foreground'
              }`}
            >
              / {column.wipLimit}
            </span>
          ) : null}
        </div>

        <button
          type="button"
          onClick={() => onCreateIssue(column.id)}
          className="rounded-md p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
          aria-label={`Add issue to ${column.name}`}
        >
          <Plus className="size-4" />
        </button>
      </div>

      {/* WIP limit warning bar */}
      {isOverWip ? (
        <div className="mx-3 mb-1 rounded bg-destructive/10 px-2 py-0.5 text-center text-[10px] font-medium text-destructive">
          WIP limit exceeded
        </div>
      ) : null}

      {/* Issues list */}
      <div className="custom-scrollbar flex flex-1 flex-col gap-2 overflow-y-auto px-2 pb-2">
        {sortedIssues.map((issue, index) => (
          <div
            key={issue.id}
            draggable={true}
            onDragStart={(e) => handleDragStart(e, issue.id)}
            onDragEnd={handleDragEnd}
            onDragOver={(e) => handleDragOverCard(e, issue.id)}
            onDragLeave={() => handleDragLeaveCard(issue.id)}
            onDrop={(e) => handleDropOnCard(e, issue.id, issue.position)}
            className="group/issue relative select-none"
            style={{ userSelect: 'none' }}
          >
            {hoveredCardId === issue.id && hoveredCardSide === 'top' && (
              <div className="absolute -top-1 left-0 right-0 h-0.5 bg-primary rounded-full z-10 animate-pulse" />
            )}
            
            <IssueCard
              issue={issue}
              onClick={() => onIssueClick(issue.id)}
              isDragging={draggedIssueId === issue.id}
            />
            <div className="pointer-events-none absolute right-1 top-1 flex rounded bg-card/95 opacity-0 shadow-sm transition-opacity group-hover/issue:pointer-events-auto group-hover/issue:opacity-100 group-focus-within/issue:pointer-events-auto group-focus-within/issue:opacity-100">
              <button type="button" disabled={index === 0}
                aria-label={`Move ${issue.issueKey} up in ${column.name}`}
                onClick={() => {
                  const previous = sortedIssues[index - 1];
                  if (previous) onDrop(issue.id, column.id, previous.position);
                }}
                className="rounded p-1 text-muted-foreground hover:text-foreground disabled:opacity-40"
              ><ChevronUp className="size-3.5" /></button>
              <button type="button" disabled={index === sortedIssues.length - 1}
                aria-label={`Move ${issue.issueKey} down in ${column.name}`}
                onClick={() => {
                  const next = sortedIssues[index + 1];
                  if (next) onDrop(issue.id, column.id, next.position);
                }}
                className="rounded p-1 text-muted-foreground hover:text-foreground disabled:opacity-40"
              ><ChevronDown className="size-3.5" /></button>
            </div>
            {hoveredCardId === issue.id && hoveredCardSide === 'bottom' && (
              <div className="absolute -bottom-1 left-0 right-0 h-0.5 bg-primary rounded-full z-10 animate-pulse" />
            )}
          </div>
        ))}

        {sortedIssues.length === 0 ? (
          <div
            className={`flex h-24 items-center justify-center rounded-lg border border-dashed text-xs text-muted-foreground transition-colors ${
              dragOver ? 'border-primary/40 text-primary' : 'border-border/30'
            }`}
          >
            {dragOver ? 'Drop here' : 'No issues'}
          </div>
        ) : null}
      </div>
    </div>
  );
}
