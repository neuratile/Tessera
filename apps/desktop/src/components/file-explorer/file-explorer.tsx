import { ChevronRight, File, Folder, FolderOpen, Loader2 } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { Tree, type NodeRendererProps } from 'react-arborist';

import { filesystem, getErrorMessage } from '@/lib/ipc';
import { openFileInEditor } from '@/lib/open-file';
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
  const refreshTree = useWorkspaceStore((s) => s.refreshTree);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ width: 280, height: 400 });
  const [loadingFolders, setLoadingFolders] = useState<Set<string>>(new Set());

  const projectId = project?.id;
  const projectRootPath = project?.rootPath;

  console.log("DEBUG: FileExplorer rendering, project is:", project);
  // Setup directory watch subscription when active project changes
  useEffect(() => {
    if (projectRootPath === undefined) return;

    let active = true;
    let stopWatching: (() => void) | null = null;

    void (async () => {
      try {
        const unwatch = await filesystem.watchDirectory(
          projectRootPath,
          () => {
            if (!active) return;
            void refreshTree();
          },
          {
            recursive: true,
            delayMs: 200,
          },
        );

        if (!active) {
          unwatch();
        } else {
          stopWatching = unwatch;
        }
      } catch (err) {
        console.error('Failed to start directory watcher:', err);
      }
    })();

    return () => {
      active = false;
      if (stopWatching !== null) {
        stopWatching();
      }
    };
  }, [projectId, projectRootPath, refreshTree]);

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
    if (target.isLoaded) return; // already loaded

    setLoadingFolders((prev) => {
      const next = new Set(prev);
      next.add(target.relativePath);
      return next;
    });

    setTreeError(null);
    void (async () => {
      try {
        const children = await filesystem.readDirectoryEntries(
          target.absolutePath,
          target.relativePath,
        );
        setChildren(target.relativePath, children);
      } catch (err) {
        setTreeError(getErrorMessage(err));
      } finally {
        setLoadingFolders((prev) => {
          const next = new Set(prev);
          next.delete(target.relativePath);
          return next;
        });
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
    <div className="flex h-full flex-col overflow-hidden bg-card">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-border bg-card px-3">
        <span className="text-[11px] font-semibold uppercase tracking-[0.08em] text-foreground whitespace-nowrap">
          Explorer
        </span>
        <span
          className="text-muted-foreground/60 truncate font-mono text-[10px]"
          title={project.rootPath}
        >
          {project.name}
        </span>
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
              if (first.data.kind === 'file') {
                openFileInEditor(first.data);
              }
            }}
          >
            {(props) => <NodeRow {...props} loadingFolders={loadingFolders} />}
          </Tree>
        )}
      </div>
    </div>
  );
}

function NodeRow({
  node,
  style,
  dragHandle,
  loadingFolders,
}: NodeRendererProps<FsEntry> & { loadingFolders: Set<string> }) {
  const selected = useWorkspaceStore((s) => s.selectedPath);
  const isSelected = selected === node.data.relativePath;
  const isDir = node.data.kind === 'directory';
  const isLoading = loadingFolders.has(node.data.relativePath);

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
      className={`group relative flex cursor-pointer select-none items-center gap-1.5 px-2 font-mono text-[11px] transition-colors hover:bg-muted/60 ${
        isSelected
          ? 'bg-primary/10 text-primary before:bg-primary before:absolute before:inset-y-0 before:left-0 before:w-0.5'
          : 'text-foreground'
      }`}
    >
      <span className="text-muted-foreground flex w-4 shrink-0 items-center justify-center">
        {isDir && !isLoading && (
          <ChevronRight
            className={`size-3 transition-transform duration-150 ${
              node.isOpen ? 'rotate-90' : ''
            }`}
          />
        )}
      </span>
      <span className="text-muted-foreground flex shrink-0 items-center">
        {isLoading ? (
          <Loader2 className="size-3.5 animate-spin text-primary" />
        ) : isDir ? (
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
