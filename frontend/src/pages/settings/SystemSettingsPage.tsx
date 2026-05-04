import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  useSystemSettings,
  useUpdateSystemSettings,
} from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/api'
import {
  composePickedPath,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
  type FolderPickResult,
} from '@/lib/folderPicker'

const PICKER_SCOPE = 'group-workspace-root'

export function SystemSettingsPage() {
  const settings = useSystemSettings()
  const update = useUpdateSystemSettings()
  const fallbackInputRef = useRef<HTMLInputElement | null>(null)
  const pathInputRef = useRef<HTMLInputElement | null>(null)
  const [root, setRoot] = useState('')
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (settings.data) {
      setRoot(settings.data.group_workspace_root ?? '')
    }
  }, [settings.data])

  const onRootChange = (next: string) => {
    setRoot(next)
    saveRememberedPrefix(PICKER_SCOPE, next)
  }

  const applyPick = (folderName: string) => {
    if (!folderName) return
    const remembered = readRememberedPrefix(PICKER_SCOPE)
    const composed = composePickedPath(root, folderName, remembered)
    setRoot(composed)
    saveRememberedPrefix(PICKER_SCOPE, composed)
    requestAnimationFrame(() => {
      pathInputRef.current?.focus()
    })
  }

  const onPickFolder = async () => {
    setError(null)
    const result: FolderPickResult = await pickFolder()
    if (result.kind === 'native') {
      applyPick(result.name)
      return
    }
    if (result.kind === 'cancelled') {
      return
    }
    fallbackInputRef.current?.click()
  }

  const onFallbackChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    const relative = file?.webkitRelativePath
    if (relative) {
      const folderName = relative.split('/')[0] ?? ''
      applyPick(folderName)
    }
    if (fallbackInputRef.current) {
      fallbackInputRef.current.value = ''
    }
  }

  const onSave = async () => {
    setError(null)
    try {
      const value = root.trim() ? root.trim() : null
      await update.mutateAsync({ group_workspace_root: value })
      if (value) saveRememberedPrefix(PICKER_SCOPE, value)
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Network error')
    }
  }

  const onClear = async () => {
    setError(null)
    try {
      await update.mutateAsync({ group_workspace_root: null })
      setRoot('')
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Network error')
    }
  }

  return (
    <div className="flex h-full flex-col bg-background">
      <header className="flex h-14 shrink-0 items-center border-b border-border px-6">
        <h1 className="text-base font-semibold">System settings</h1>
      </header>
      <main className="flex-1 overflow-y-auto p-6">
        <div className="mx-auto max-w-2xl space-y-6">
          <section className="rounded-lg border border-border bg-card p-5">
            <h2 className="text-sm font-semibold">Group workspace root</h2>
            <div className="mt-4 space-y-2">
              <Label htmlFor="ss-root">Local directory</Label>
              <div className="flex gap-2">
                <Input
                  id="ss-root"
                  ref={pathInputRef}
                  value={root}
                  onChange={(event) => onRootChange(event.target.value)}
                  placeholder="D:/workspaces/groups"
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void onPickFolder()}
                >
                  Pick folder
                </Button>
              </div>
              <input
                ref={fallbackInputRef}
                type="file"
                className="hidden"
                multiple
                {...({
                  webkitdirectory: '',
                  directory: '',
                } as Record<string, string>)}
                onChange={onFallbackChange}
              />
            </div>

            {error ? <p className="mt-3 text-sm text-red-600">{error}</p> : null}

            <div className="mt-4 flex items-center gap-2">
              <Button onClick={onSave} disabled={update.isPending}>
                {update.isPending ? 'Saving…' : 'Save'}
              </Button>
              <Button variant="outline" onClick={onClear} disabled={update.isPending}>
                Clear
              </Button>
            </div>
          </section>
        </div>
      </main>
    </div>
  )
}
