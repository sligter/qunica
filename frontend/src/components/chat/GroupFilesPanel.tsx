import { useRef } from 'react'
import { File, Trash2, Upload } from 'lucide-react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useDeleteGroupFile, useGroupFiles, useUploadGroupFile } from '@/hooks/useGroupFiles'

interface GroupFilesPanelProps {
  groupId: string
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function GroupFilesPanel({ groupId, open, onOpenChange }: GroupFilesPanelProps) {
  const files = useGroupFiles(groupId)
  const upload = useUploadGroupFile(groupId)
  const del = useDeleteGroupFile(groupId)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    await upload.mutateAsync(file)
    if (fileInputRef.current) fileInputRef.current.value = ''
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg max-h-[80vh] overflow-y-auto">
        <DialogHeader>
          <div className="flex items-center justify-between pr-8">
            <DialogTitle>Group Files</DialogTitle>
            <Button
              size="sm"
              variant="outline"
              onClick={() => fileInputRef.current?.click()}
              disabled={upload.isPending}
            >
              <Upload className="mr-1 h-3.5 w-3.5" />
              {upload.isPending ? 'Uploading…' : 'Upload'}
            </Button>
          </div>
        </DialogHeader>

        <input
          ref={fileInputRef}
          type="file"
          className="hidden"
          onChange={handleUpload}
        />

        {files.isLoading && (
          <p className="text-sm text-muted-foreground">Loading files…</p>
        )}

        {files.data && files.data.length === 0 && (
          <div className="flex flex-col items-center gap-2 py-10 text-center">
            <File className="h-8 w-8 text-muted-foreground" />
            <p className="text-sm text-muted-foreground">No files yet</p>
          </div>
        )}

        {files.data && files.data.length > 0 && (
          <ul className="divide-y divide-border">
            {files.data.map((f) => (
              <li key={f.id} className="flex items-center justify-between gap-3 py-2.5">
                <div className="flex min-w-0 items-center gap-2">
                  <File className="h-4 w-4 shrink-0 text-muted-foreground" />
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium">{f.filename}</p>
                    <p className="text-[10px] text-muted-foreground">
                      {(f.file_size / 1024).toFixed(1)} KB
                      {f.mime_type && ` · ${f.mime_type}`}
                    </p>
                  </div>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  className="h-7 w-7 shrink-0 text-muted-foreground hover:text-red-600"
                  onClick={() => del.mutate(f.id)}
                  disabled={del.isPending}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </Button>
              </li>
            ))}
          </ul>
        )}
      </DialogContent>
    </Dialog>
  )
}
