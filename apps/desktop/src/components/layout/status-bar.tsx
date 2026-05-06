import { useWorkspaceStore } from '@/stores/workspace-store';

/**
 * Bottom status bar. Phase 9 surfaces project status + selected file
 * + analyse-time stats once Phase 11 wires `analyze_project` to it.
 * For now: project name + file count + selected path.
 */
export function StatusBar() {
  const project = useWorkspaceStore((s) => s.project);
  const selectedPath = useWorkspaceStore((s) => s.selectedPath);
  const treeError = useWorkspaceStore((s) => s.treeError);

  return (
    <footer className="flex h-6 shrink-0 items-center justify-between gap-2 border-t border-border bg-card px-3 text-xs">
      <div className="flex items-center gap-3">
        {project ? (
          <>
            <span className="text-muted-foreground">{project.status}</span>
            <span className="text-muted-foreground">{project.fileCount} files</span>
          </>
        ) : (
          <span className="text-muted-foreground">no project</span>
        )}
      </div>
      <div className="flex items-center gap-3">
        {treeError !== null ? (
          <span className="text-destructive truncate" role="alert" title={treeError}>
            {treeError}
          </span>
        ) : null}
        {selectedPath !== null ? (
          <code className="text-muted-foreground truncate">{selectedPath}</code>
        ) : null}
      </div>
    </footer>
  );
}
