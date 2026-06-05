/* eslint-disable */
import { useState } from 'react';
import { X, Save, Trash2, Plus, ArrowUp, ArrowDown, Loader2 } from 'lucide-react';

import { useBoardStore } from '@/stores/board-store';
import { updateBoard, updateColumn, createColumn, deleteColumn, reorderColumns } from '@/lib/ipc/boards';

type Props = {
  onClose: () => void;
};

export function BoardSettings({ onClose }: Props) {
  const activeBoardId = useBoardStore((s) => s.activeBoardId);
  const activeTeamId = useBoardStore((s) => s.activeTeamId);
  const boards = useBoardStore((s) => s.boards);
  const columns = useBoardStore((s) => s.columns);
  
  const setBoards = useBoardStore((s) => s.setBoards);
  const setColumns = useBoardStore((s) => s.setColumns);

  const activeBoard = boards.find((b) => b.id === activeBoardId);

  // Board details state
  const [boardName, setBoardName] = useState(activeBoard?.name || '');
  const [boardDesc, setBoardDesc] = useState(activeBoard?.description || '');
  const [savingBoard, setSavingBoard] = useState(false);

  // Column edit states
  const [newColName, setNewColName] = useState('');
  const [newColColor, setNewColColor] = useState('#6b7280');
  const [newColWip, setNewColWip] = useState('');
  
  const [editingColId, setEditingColId] = useState<string | null>(null);
  const [editColName, setEditColName] = useState('');
  const [editColColor, setEditColColor] = useState('#6b7280');
  const [editColWip, setEditColWip] = useState('');

  const [loading, setLoading] = useState(false);

  const handleUpdateBoardDetails = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeBoardId || !activeTeamId || !boardName.trim()) return;

    setSavingBoard(true);
    try {
      const payload: any = { name: boardName.trim() };
      if (boardDesc.trim()) {
        payload.description = boardDesc.trim();
      }
      const updated = await updateBoard(activeBoardId, payload);

      // Update boards list
      setBoards(boards.map((b) => (b.id === activeBoardId ? updated : b)));
    } catch (err) {
      console.error('Failed to update board details:', err);
    } finally {
      setSavingBoard(false);
    }
  };

  const handleAddColumn = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!activeBoardId || !newColName.trim()) return;

    setLoading(true);
    try {
      const payload: any = {
        name: newColName.trim(),
        color: newColColor,
      };
      if (newColWip) {
        payload.wipLimit = parseInt(newColWip, 10);
      }
      const created = await createColumn(activeBoardId, payload);

      setColumns([...columns, created]);
      setNewColName('');
      setNewColWip('');
    } catch (err) {
      console.error('Failed to add column:', err);
    } finally {
      setLoading(false);
    }
  };

  const handleStartEditColumn = (col: any) => {
    setEditingColId(col.id);
    setEditColName(col.name);
    setEditColColor(col.color);
    setEditColWip(col.wipLimit !== undefined && col.wipLimit !== null ? col.wipLimit.toString() : '');
  };

  const handleSaveEditColumn = async (columnId: string) => {
    if (!editColName.trim()) return;
    setLoading(true);
    try {
      const payload: any = {
        name: editColName.trim(),
        color: editColColor,
      };
      if (editColWip) {
        payload.wipLimit = parseInt(editColWip, 10);
      } else {
        payload.wipLimit = null;
      }
      const updated = await updateColumn(columnId, payload);

      setColumns(columns.map((c) => (c.id === columnId ? updated : c)));
      setEditingColId(null);
    } catch (err) {
      console.error('Failed to save column details:', err);
    } finally {
      setLoading(false);
    }
  };

  const handleDeleteColumn = async (columnId: string) => {
    if (columns.length <= 1) {
      alert('A board must have at least one column.');
      return;
    }
    if (!confirm('Are you sure you want to delete this column? Its issues will move to the leftmost remaining column.')) {
      return;
    }

    setLoading(true);
    try {
      await deleteColumn(columnId);
      setColumns(columns.filter((c) => c.id !== columnId));
    } catch (err) {
      console.error('Failed to delete column:', err);
    } finally {
      setLoading(false);
    }
  };

  const handleMoveColumn = async (index: number, direction: 'up' | 'down') => {
    if (!activeBoardId) return;
    
    const reordered = [...columns].sort((a, b) => a.position - b.position);
    const targetIndex = direction === 'up' ? index - 1 : index + 1;
    
    if (targetIndex < 0 || targetIndex >= reordered.length) return;

    // Swap elements
    const temp = reordered[index];
    const targetCol = reordered[targetIndex];
    if (temp && targetCol) {
      reordered[index] = targetCol;
      reordered[targetIndex] = temp;
    }

    setLoading(true);
    try {
      const ids = reordered.map((c) => c.id);
      const updated = await reorderColumns(activeBoardId, ids);
      setColumns(updated);
    } catch (err) {
      console.error('Failed to reorder columns:', err);
    } finally {
      setLoading(false);
    }
  };

  if (!activeBoard) return null;

  const sortedColumns = [...columns].sort((a, b) => a.position - b.position);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-xs">
      <div className="flex h-[90vh] w-full max-w-2xl flex-col rounded-xl border border-border bg-surface-2 shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border/40 px-6 py-4">
          <h3 className="text-base font-bold text-foreground">Board Configuration</h3>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md p-1 text-muted-foreground hover:bg-muted hover:text-foreground transition-colors"
          >
            <X className="size-4.5" />
          </button>
        </div>

        {/* Scrollable content */}
        <div className="custom-scrollbar flex-1 overflow-y-auto p-6 space-y-6">
          {/* Board Detail Form */}
          <form onSubmit={handleUpdateBoardDetails} className="space-y-4 rounded-lg border border-border/50 bg-background/20 p-4">
            <h4 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
              General Details
            </h4>
            
            <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
              <div className="space-y-1.5 sm:col-span-2">
                <label htmlFor="settings-name" className="text-[10px] font-bold uppercase tracking-wider text-muted-foreground">
                  Board Name
                </label>
                <input
                  id="settings-name"
                  type="text"
                  required
                  value={boardName}
                  onChange={(e) => setBoardName(e.target.value)}
                  className="h-9 w-full rounded-lg border border-border/60 bg-background px-3 text-xs text-foreground outline-none focus:border-primary transition-all"
                />
              </div>

              <div className="space-y-1.5">
                <label className="text-[10px] font-bold uppercase tracking-wider text-muted-foreground block">
                  Board Key
                </label>
                <input
                  type="text"
                  readOnly
                  disabled
                  value={activeBoard.key}
                  className="h-9 w-full rounded-lg border border-border/60 bg-muted/30 px-3 text-xs text-muted-foreground font-mono outline-none cursor-not-allowed"
                />
              </div>
            </div>

            <div className="space-y-1.5">
              <label htmlFor="settings-desc" className="text-[10px] font-bold uppercase tracking-wider text-muted-foreground">
                Description
              </label>
              <textarea
                id="settings-desc"
                rows={2}
                value={boardDesc}
                onChange={(e) => setBoardDesc(e.target.value)}
                className="w-full rounded-lg border border-border/60 bg-background p-2.5 text-xs text-foreground outline-none focus:border-primary transition-all resize-none"
              />
            </div>

            <div className="flex justify-end">
              <button
                type="submit"
                disabled={savingBoard || boardName.trim() === activeBoard.name && boardDesc.trim() === (activeBoard.description || '')}
                className="flex h-9 items-center gap-1.5 rounded-lg bg-primary hover:bg-primary/90 px-4 text-xs font-semibold text-primary-foreground shadow-md shadow-primary/10 transition-colors disabled:opacity-40"
              >
                {savingBoard ? <Loader2 className="size-3.5 animate-spin" /> : <Save className="size-3.5" />}
                Save Details
              </button>
            </div>
          </form>

          {/* Columns Config */}
          <div className="space-y-4">
            <h4 className="text-xs font-bold uppercase tracking-wider text-muted-foreground">
              Columns Layout & WIP limits
            </h4>

            {/* List columns */}
            <div className="border border-border/40 bg-surface-1/20 rounded-lg p-2 space-y-1">
              {sortedColumns.map((col, index) => {
                const isEditing = editingColId === col.id;
                return (
                  <div key={col.id} className="flex items-center justify-between rounded-lg p-2 hover:bg-background/20 transition-all border border-transparent hover:border-border/30">
                    {isEditing ? (
                      <div className="flex flex-1 flex-wrap items-center gap-2">
                        <input
                          type="color"
                          value={editColColor}
                          onChange={(e) => setEditColColor(e.target.value)}
                          className="size-7 rounded cursor-pointer border border-border"
                        />
                        <input
                          type="text"
                          required
                          value={editColName}
                          onChange={(e) => setEditColName(e.target.value)}
                          className="h-8 flex-1 rounded border border-border bg-background px-2 text-xs text-foreground outline-none"
                        />
                        <input
                          type="number"
                          value={editColWip}
                          onChange={(e) => setEditColWip(e.target.value)}
                          placeholder="WIP"
                          className="h-8 w-16 rounded border border-border bg-background px-2 text-xs text-foreground outline-none"
                        />
                        <button
                          type="button"
                          onClick={() => handleSaveEditColumn(col.id)}
                          className="h-8 rounded bg-primary px-3 text-xs font-semibold text-primary-foreground hover:bg-primary/90 transition-colors"
                        >
                          Save
                        </button>
                        <button
                          type="button"
                          onClick={() => setEditingColId(null)}
                          className="h-8 rounded border border-border px-3 text-xs text-muted-foreground hover:bg-muted"
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <>
                        <div className="flex items-center gap-2.5 overflow-hidden">
                          <div
                            className="size-3 rounded-full shrink-0"
                            style={{ backgroundColor: col.color }}
                          />
                          <span className="text-xs font-semibold text-foreground truncate">{col.name}</span>
                          {col.wipLimit !== undefined && col.wipLimit !== null ? (
                            <span className="text-[10px] font-mono opacity-60 bg-muted px-1.5 rounded">
                              WIP: {col.wipLimit}
                            </span>
                          ) : null}
                        </div>

                        <div className="flex items-center gap-1.5 shrink-0">
                          {/* Order buttons */}
                          <button
                            type="button"
                            disabled={index === 0 || loading}
                            onClick={() => handleMoveColumn(index, 'up')}
                            className="rounded p-1 text-muted-foreground hover:bg-muted disabled:opacity-30 transition-colors"
                          >
                            <ArrowUp className="size-3.5" />
                          </button>
                          <button
                            type="button"
                            disabled={index === sortedColumns.length - 1 || loading}
                            onClick={() => handleMoveColumn(index, 'down')}
                            className="rounded p-1 text-muted-foreground hover:bg-muted disabled:opacity-30 transition-colors"
                          >
                            <ArrowDown className="size-3.5" />
                          </button>

                          <button
                            type="button"
                            onClick={() => handleStartEditColumn(col)}
                            className="text-xs font-semibold text-primary hover:underline px-2.5 py-1"
                          >
                            Edit
                          </button>

                          <button
                            type="button"
                            disabled={loading}
                            onClick={() => handleDeleteColumn(col.id)}
                            className="rounded p-1 text-muted-foreground hover:bg-destructive/15 hover:text-destructive transition-colors"
                          >
                            <Trash2 className="size-3.5" />
                          </button>
                        </div>
                      </>
                    )}
                  </div>
                );
              })}
            </div>

            {/* Create column form */}
            <form onSubmit={handleAddColumn} className="flex flex-wrap items-center gap-2 border border-border/50 bg-background/10 rounded-lg p-3">
              <input
                type="color"
                value={newColColor}
                onChange={(e) => setNewColColor(e.target.value)}
                className="size-8 rounded cursor-pointer border border-border"
                title="Column color tag"
              />
              <input
                type="text"
                required
                value={newColName}
                onChange={(e) => setNewColName(e.target.value)}
                placeholder="New status name (e.g. In Review)"
                className="h-8 flex-1 rounded border border-border bg-background px-2.5 text-xs text-foreground outline-none"
              />
              <input
                type="number"
                value={newColWip}
                onChange={(e) => setNewColWip(e.target.value)}
                placeholder="WIP limit"
                className="h-8 w-20 rounded border border-border bg-background px-2.5 text-xs text-foreground outline-none"
              />
              <button
                type="submit"
                disabled={loading || !newColName.trim()}
                className="flex h-8 items-center gap-1.5 rounded bg-primary hover:bg-primary/90 px-3 text-xs font-semibold text-primary-foreground shadow-md shadow-primary/10 transition-colors disabled:opacity-50"
              >
                <Plus className="size-3.5" />
                Add Column
              </button>
            </form>
          </div>
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-3 border-t border-border/40 px-6 py-4 bg-background/25">
          <button
            type="button"
            onClick={onClose}
            className="h-9 rounded-lg bg-primary hover:bg-primary/90 px-4 text-xs font-semibold text-primary-foreground shadow-md shadow-primary/10 transition-colors"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
