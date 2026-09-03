/**
 * Folder picker for the web build.
 *
 * The OS folder dialog only ever shows the machine running the browser. When
 * the backend lives somewhere else — a container, a VPS — that machine's
 * folders are the wrong answer entirely, so this browses the server's workspace
 * root instead and returns a path the backend can actually open.
 */

import { ChevronRight, CornerLeftUp, FolderOpen, HardDrive } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import {
  useCreateWorkspaceDirectory,
  useWorkspaceDirectories,
} from '@/hooks/useWorkspaceDirectories'
import { ApiError } from '@/lib/api-v2/client'

export interface ServerFolderPickerProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  /**
   * Receives the absolute path on the server. Callers that would rather store a
   * name relative to the workspace root get that as the second argument.
   */
  onSelect: (absolutePath: string, relativePath: string) => void
}

/**
 * Mount this only while it is open. It queries the server as soon as it exists,
 * and a closed dialog has no business holding a query — or forcing every screen
 * that merely *offers* a folder button to sit inside a QueryClientProvider.
 */
export function ServerFolderPicker({ open, onOpenChange, onSelect }: ServerFolderPickerProps) {
  const { t } = useTranslation(['workspaces', 'common'])
  const [path, setPath] = useState('')
  const [newFolder, setNewFolder] = useState('')
  const listing = useWorkspaceDirectories(path, open)
  const createDirectory = useCreateWorkspaceDirectory()

  // Every visit starts at the root: a stale path from a previous open may not
  // exist any more, and the request would just 404.
  useEffect(() => {
    if (open) {
      setPath('')
      setNewFolder('')
    }
  }, [open])

  const data = listing.data
  const rootRequired =
    listing.error instanceof ApiError && listing.error.code === 'workspace_root_required'

  const chooseCurrent = () => {
    if (!data) return
    onSelect(data.absolute_path, data.relative_path)
    onOpenChange(false)
  }

  const createFolder = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    const name = newFolder.trim()
    if (!data || !name) return
    createDirectory.mutate(
      { parent: data.relative_path, name },
      {
        onSuccess: (directory) => {
          setNewFolder('')
          setPath(directory.relative_path)
        },
      },
    )
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{t('workspaces:serverPicker.title')}</DialogTitle>
          <DialogDescription>{t('workspaces:serverPicker.description')}</DialogDescription>
        </DialogHeader>

        {rootRequired ? (
          <p className="rounded-lg bg-muted/40 p-3 text-xs text-muted-foreground" role="status">
            {t('workspaces:serverPicker.rootRequired')}
          </p>
        ) : (
          <div className="space-y-2">
            <div
              className="flex items-center gap-1.5 truncate rounded-md bg-muted/40 px-2.5 py-2 font-mono text-xs text-muted-foreground"
              title={data?.absolute_path}
            >
              <HardDrive className="h-3.5 w-3.5 shrink-0" />
              <span className="truncate">{data?.absolute_path ?? t('common:state.loading')}</span>
            </div>

            <ScrollArea className="h-56 rounded-md border border-border">
              <div className="p-1">
                {data && data.parent_relative_path !== null ? (
                  <button
                    type="button"
                    className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-muted"
                    onClick={() => setPath(data.parent_relative_path ?? '')}
                  >
                    <CornerLeftUp className="h-4 w-4 shrink-0 text-muted-foreground" />
                    {t('workspaces:serverPicker.parent')}
                  </button>
                ) : null}

                {listing.isLoading ? (
                  <p className="px-2 py-1.5 text-sm text-muted-foreground">
                    {t('common:state.loading')}
                  </p>
                ) : null}

                {listing.isError && !rootRequired ? (
                  <p className="px-2 py-1.5 text-sm text-destructive" role="alert">
                    {listing.error instanceof Error
                      ? listing.error.message
                      : t('workspaces:serverPicker.loadFailed')}
                  </p>
                ) : null}

                {data?.entries.map((entry) => (
                  <button
                    key={entry.relative_path}
                    type="button"
                    className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-muted"
                    onClick={() => setPath(entry.relative_path)}
                    onDoubleClick={() => {
                      onSelect(entry.absolute_path, entry.relative_path)
                      onOpenChange(false)
                    }}
                  >
                    <FolderOpen className="h-4 w-4 shrink-0 text-muted-foreground" />
                    <span className="truncate">{entry.name}</span>
                    <ChevronRight className="ml-auto h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                  </button>
                ))}

                {data && data.entries.length === 0 && !listing.isLoading ? (
                  <p className="px-2 py-1.5 text-sm text-muted-foreground">
                    {t('workspaces:serverPicker.empty')}
                  </p>
                ) : null}
              </div>
            </ScrollArea>

            {data?.truncated ? (
              <p className="text-2xs text-muted-foreground">
                {t('workspaces:serverPicker.truncated')}
              </p>
            ) : null}

            <form className="flex gap-2" onSubmit={createFolder}>
              <Input
                value={newFolder}
                onChange={(event) => setNewFolder(event.target.value)}
                placeholder={t('workspaces:serverPicker.newFolderPlaceholder')}
                aria-label={t('workspaces:serverPicker.newFolderPlaceholder')}
                disabled={!data || createDirectory.isPending}
              />
              <Button
                type="submit"
                variant="outline"
                disabled={!data || !newFolder.trim() || createDirectory.isPending}
              >
                {t('workspaces:serverPicker.createFolder')}
              </Button>
            </form>
            {createDirectory.isError ? (
              <p className="text-xs text-destructive" role="alert">
                {createDirectory.error instanceof Error
                  ? createDirectory.error.message
                  : t('workspaces:serverPicker.createFailed')}
              </p>
            ) : null}
          </div>
        )}

        <DialogFooter>
          <Button type="button" variant="outline" onClick={() => onOpenChange(false)}>
            {t('common:actions.cancel')}
          </Button>
          <Button type="button" disabled={!data} onClick={chooseCurrent}>
            {t('workspaces:serverPicker.useThisFolder')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
