
import { AppShell } from '@/components/layout/AppShell'
import { FileExplorer } from '@/components/features/file-explorer/FileExplorer'
import { EditorPanel } from '@/components/features/editor/EditorPanel'
import { AiActionPanel } from '@/components/features/ai-panel/AiActionPanel'
import { FolderUpload } from '@/components/features/folder-upload/FolderUpload'
import { SettingsSheet } from '@/components/features/settings/SettingsSheet'

export function MainPage() {
  return (
    <>
      <AppShell
        sidebar={<FileExplorer />}
        editor={<EditorPanel />}
        aiPanel={<AiActionPanel />}
      />
      <FolderUpload />
      <SettingsSheet />
    </>
  )
}
