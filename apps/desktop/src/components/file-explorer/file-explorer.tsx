import { ChevronDown, ChevronRight, File, Folder, FolderOpen } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Tree, type NodeRendererProps } from 'react-arborist';

import { filesystem, IpcError } from '@/lib/ipc';
import { useWorkspaceStore, type FsEntry } from '@/stores/workspace-store';

/**
 * Left sidebar: project file tree.
 *
 * Lazy-loads directories on expand via `filesystem.readDirectoryEntries`.
 * Empty `children: []` on a directory entry signals "expandable but
 * unloaded" to react-arborist; we replace it on first expand.
 */
export function FileExplorer() {
  const project = useWorkspaceStore((s) => s.project);
  const tree = useWorkspaceStore((s) => s.tree);
  const loading = useWorkspaceStore((s) => s.loadingTree);
  const error = useWorkspaceStore((s) => s.treeError);
  const setSelectedPath = useWorkspaceStore((s) => s.setSelectedPath);
  const setChildren = useWorkspaceStore((s) => s.setChildren);
  const setTreeError = useWorkspaceStore((s) => s.setTreeError);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 280, height: 400 });

  useEffect(() => {
    const node = containerRef.current;
    if (node === null) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry === undefined) return;
      const { width, height } = entry.contentRect;
      setSize({ width, height });
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const handleToggle = (id: string) => {
    const target = findEntry(tree, id);
    if (target === null || target.kind !== 'directory') return;
    if (target.children !== undefined && target.children.length > 0) return; // already loaded
    setTreeError(null);
    void (async () => {
      try {
        const children = await filesystem.readDirectoryEntries(
          target.absolutePath,
          target.relativePath,
        );
        setChildren(target.relativePath, children);
      } catch (err) {
        setTreeError(err instanceof IpcError ? err.message : String(err));
      }
    })();
  };

  if (project === null) {
    return (
      <div className="text-muted-foreground flex flex-1 items-center justify-center px-4 text-center text-sm">
        No project open. Use “Open folder” in the toolbar.
      </div>
    );
  }

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="border-b border-border px-3 py-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
        Explorer
      </div>
      <div ref={containerRef} className="flex-1 overflow-hidden">
        {loading && tree.length === 0 ? (
          <p className="text-muted-foreground p-3 text-sm">Reading directory…</p>
        ) : error !== null && tree.length === 0 ? (
          <p className="text-destructive p-3 text-sm" role="alert">
            {error}
          </p>
        ) : tree.length === 0 ? (
          <p className="text-muted-foreground p-3 text-sm">Empty folder.</p>
        ) : (
          <Tree<FsEntry>
            data={tree}
            width={size.width}
            height={size.height}
            rowHeight={24}
            indent={16}
            onToggle={handleToggle}
            onSelect={(nodes) => {
              const first = nodes[0];
              if (first === undefined) return;
              setSelectedPath(first.data.relativePath);
            }}
          >
            {NodeRow}
          </Tree>
        )}
      </div>
    </div>
  );
}

function NodeRow({ node, style, dragHandle }: NodeRendererProps<FsEntry>) {
  const selected = useWorkspaceStore((s) => s.selectedPath);
  const isSelected = selected === node.data.relativePath;
  const isDir = node.data.kind === 'directory';

  return (
    <div
      ref={dragHandle}
      style={style}
      onClick={() => {
        if (isDir) {
          node.toggle();
        } else {
          node.select();
        }
      }}
      className={`group flex cursor-pointer select-none items-center gap-1 px-2 text-xs hover:bg-muted/50 ${
        isSelected ? 'bg-primary/10 text-primary' : 'text-foreground'
      }`}
    >
      <span className="text-muted-foreground flex w-4 shrink-0 items-center justify-center">
        {isDir ? (
          node.isOpen ? (
            <ChevronDown className="size-3" />
          ) : (
            <ChevronRight className="size-3" />
          )
        ) : null}
      </span>
      <span className="text-muted-foreground flex shrink-0 items-center">
        {isDir ? (
          node.isOpen ? (
            <FolderOpen className="size-3.5" />
          ) : (
            <Folder className="size-3.5" />
          )
        ) : (
          <File className="size-3.5" />
        )}
      </span>
      <span className="truncate">{node.data.name}</span>
    </div>
  );
}

function findEntry(entries: FsEntry[], id: string): FsEntry | null {
  for (const entry of entries) {
    if (entry.id === id) return entry;
    if (entry.children) {
      const found = findEntry(entry.children, id);
      if (found !== null) return found;
    }
  }
  return null;
}
