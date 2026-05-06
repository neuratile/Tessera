
import { Tree, type NodeRendererProps } from 'react-arborist'
import { FileCode2, Folder, FolderOpen, ChevronRight, ChevronDown } from 'lucide-react'
import { useProjectStore, type ProjectFile } from '@/stores/project-store'
import { cn } from '@/lib/utils'

export function FileExplorer() {
  const { uploadState, openFile } = useProjectStore()

  if (uploadState.status !== 'ready') {
    return (
      <div className="flex-1 flex items-center justify-center text-sm text-muted-foreground p-4 text-center">
        {uploadState.status === 'idle' && 'No folder opened.'}
        {uploadState.status === 'uploading' && 'Uploading folder...'}
        {uploadState.status === 'error' && 'Error opening folder.'}
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full overflow-hidden text-sm">
      <div className="px-3 py-2 font-semibold text-xs tracking-wider uppercase text-muted-foreground shrink-0 bg-background/50 backdrop-blur-sm sticky top-0 z-10 border-b border-border/50">
        EXPLORER
      </div>
      <div className="flex-1 overflow-auto py-2">
        <Tree
          data={uploadState.files}
          openByDefault={false}
          width="100%"
          height={1000} // Let css handle actual height
          rowHeight={24}
          indent={16}
          paddingTop={0}
          paddingBottom={0}
          onSelect={(nodes) => {
            if (nodes.length > 0) {
              const node = nodes[0]
              if (node && node.data.type === 'file') {
                openFile(node.data)
              }
            }
          }}
        >
          {NodeRenderer}
        </Tree>
      </div>
    </div>
  )
}

function NodeRenderer({ node, style, dragHandle }: NodeRendererProps<ProjectFile>) {
  const { selectedFilePath } = useProjectStore()
  if (!node) return null
  const isSelected = selectedFilePath === node.data.path
  
  return (
    <div
      style={style}
      ref={dragHandle}
      onClick={() => node.isInternal ? node.toggle() : node.select()}
      className={cn(
        "flex items-center px-2 py-0.5 cursor-pointer rounded-sm hover:bg-muted/50 text-foreground transition-colors group select-none whitespace-nowrap",
        isSelected && "bg-primary/10 text-primary hover:bg-primary/20 font-medium"
      )}
    >
      <div className="flex items-center justify-center w-4 h-4 shrink-0 mr-1 text-muted-foreground group-hover:text-foreground transition-colors">
        {node.isInternal ? (
          node.isOpen ? <ChevronDown className="w-3.5 h-3.5" /> : <ChevronRight className="w-3.5 h-3.5" />
        ) : null}
      </div>

      <div className="flex items-center gap-1.5 min-w-0 flex-1">
        {node.isInternal ? (
          node.isOpen ? (
            <FolderOpen className="w-4 h-4 text-blue-400 shrink-0" fill="currentColor" fillOpacity={0.2} />
          ) : (
            <Folder className="w-4 h-4 text-blue-400 shrink-0" fill="currentColor" fillOpacity={0.2} />
          )
        ) : (
          <FileCode2 className="w-4 h-4 text-muted-foreground shrink-0" />
        )}
        <span className="truncate">{node.data.name}</span>
      </div>
    </div>
  )
}
