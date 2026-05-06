import { useCallback, useRef } from 'react'
import { UploadCloud } from 'lucide-react'
import { useProjectStore, type ProjectFile } from '@/stores/project-store'

export function FolderUpload() {
  const { uploadState, setUploadState } = useProjectStore()
  const inputRef = useRef<HTMLInputElement>(null)

  const handleFiles = async (fileList: FileList | File[]) => {
    const files = Array.from(fileList)
    if (files.length === 0) return

    setUploadState({ status: 'uploading', progress: 0 })

    const root: ProjectFile[] = []
    const projectName = (files[0] && files[0].webkitRelativePath ? files[0].webkitRelativePath.split('/')[0] : 'Project') || 'Project'

    // Build the tree
    for (const file of files) {
      const pathParts = file.webkitRelativePath.split('/').slice(1) // Remove root folder name
      if (pathParts.length === 0) continue

      let currentLevel = root
      let currentPath = ''

      for (let i = 0; i < pathParts.length; i++) {
        const part = pathParts[i]!
        currentPath = currentPath ? `${currentPath}/${part}` : part
        const isLast = i === pathParts.length - 1

        let existing = currentLevel.find(item => item.name === part)

        if (!existing) {
          const newItem: ProjectFile = {
            id: currentPath,
            name: part,
            path: currentPath,
            type: isLast ? 'file' : 'directory',
            language: isLast ? getLanguageFromFilename(part) : null,
            children: isLast ? undefined : []
          }

          if (isLast) {
            // Read content
            newItem.content = await readFileContent(file)
          }

          currentLevel.push(newItem)
          existing = newItem
        }

        if (!isLast && existing.children) {
          currentLevel = existing.children
        }
      }
    }

    // Sort: Folders first, then files
    const sortTree = (items: ProjectFile[]) => {
      items.sort((a, b) => {
        if (a.type === b.type) return a.name.localeCompare(b.name)
        return a.type === 'directory' ? -1 : 1
      })
      items.forEach(item => {
        if (item.children) sortTree(item.children)
      })
    }

    sortTree(root)

    setUploadState({
      status: 'ready',
      projectName,
      files: root
    })
  }

  const readFileContent = (file: File): Promise<string> => {
    return new Promise((resolve) => {
      const reader = new FileReader()
      reader.onload = (e) => resolve(e.target?.result as string || '')
      reader.onerror = () => resolve('')
      reader.readAsText(file)
    })
  }

  const getLanguageFromFilename = (filename: string): string | null => {
    const ext = filename.split('.').pop()?.toLowerCase()
    const map: Record<string, string> = {
      'ts': 'typescript', 'tsx': 'typescript',
      'js': 'javascript', 'jsx': 'javascript',
      'json': 'json', 'html': 'html', 'css': 'css',
      'md': 'markdown', 'py': 'python', 'rs': 'rust'
    }
    return map[ext || ''] || null
  }

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault()
    if (e.dataTransfer.items && e.dataTransfer.items.length > 0) {
      // For real implementation, need recursive read of DataTransferItem
      // Here we just use the mocked tree anyway.
      handleFiles(e.dataTransfer.files)
    }
  }, [])

  const handleDragOver = (e: React.DragEvent) => {
    e.preventDefault()
  }

  if (uploadState.status === 'ready' || uploadState.status === 'uploading') {
    return null
  }

  return (
    <div 
      className="absolute inset-0 z-50 flex items-center justify-center bg-background/95 backdrop-blur p-4"
      onDrop={handleDrop}
      onDragOver={handleDragOver}
    >
      <div className="max-w-md w-full border-2 border-dashed border-border rounded-xl p-10 flex flex-col items-center justify-center text-center bg-card">
        <div className="w-16 h-16 bg-primary/10 text-primary rounded-full flex items-center justify-center mb-4">
          <UploadCloud className="w-8 h-8" />
        </div>
        <h3 className="text-lg font-semibold text-foreground mb-2">Open Project Folder</h3>
        <p className="text-sm text-muted-foreground mb-6">
          Drag and drop your codebase here, or click below to select a folder from your computer.
        </p>
        
        <input 
          type="file" 
          ref={inputRef}
          className="hidden" 
          // @ts-ignore - webkitdirectory is non-standard but works in all modern browsers
          webkitdirectory="true" 
          directory="true"
          onChange={(e) => {
            if (e.target.files) handleFiles(e.target.files)
          }}
        />
        
        <button 
          onClick={() => inputRef.current?.click()}
          className="bg-primary text-primary-foreground hover:bg-primary/90 px-6 py-2.5 rounded-md font-medium transition-colors"
        >
          Select Folder
        </button>
      </div>
    </div>
  )
}
