import { useCallback, useRef } from 'react'
import { UploadCloud } from 'lucide-react'
import { useProjectStore, type ProjectFile } from '@/stores/project-store'

export function FolderUpload() {
  const { uploadState, setUploadState } = useProjectStore()
  const inputRef = useRef<HTMLInputElement>(null)

  const handleFiles = (fileList: FileList | File[]) => {
    const files = Array.from(fileList)
    if (files.length === 0) return

    setUploadState({ status: 'uploading', progress: 0 })

    // Simulate parsing
    setTimeout(() => {
      // A simple mock for folder tree building
      // We assume files come with webkitRelativePath
      const firstFile = files[0]
      const projectName = (firstFile && firstFile.webkitRelativePath ? firstFile.webkitRelativePath.split('/')[0] : 'Project') || 'Project'

      const mockTree: ProjectFile[] = [
        {
          id: 'src',
          name: 'src',
          path: 'src',
          type: 'directory',
          language: null,
          children: [
            {
              id: 'src/main.tsx',
              name: 'main.tsx',
              path: 'src/main.tsx',
              type: 'file',
              language: 'typescript',
              content: "console.log('hello world')"
            },
            {
              id: 'src/utils.ts',
              name: 'utils.ts',
              path: 'src/utils.ts',
              type: 'file',
              language: 'typescript',
              content: "export const add = (a: number, b: number) => a + b;"
            }
          ]
        },
        {
          id: 'package.json',
          name: 'package.json',
          path: 'package.json',
          type: 'file',
          language: 'json',
          content: "{\n  \"name\": \"mock-project\"\n}"
        }
      ]

      setUploadState({
        status: 'ready',
        projectName,
        files: mockTree
      })
    }, 1000)
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
