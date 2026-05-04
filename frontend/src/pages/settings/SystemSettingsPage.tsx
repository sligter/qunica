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
  const [info, setInfo] = useState<string | null>(null)
  const [pickerHint, setPickerHint] = useState<string | null>(null)

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
    setPickerHint(
      remembered
        ? `Picked "${folderName}". Combined with your remembered prefix into "${composed}". Edit if the absolute prefix is different.`
        : `Picked "${folderName}". Browsers cannot return an absolute path; please prepend the absolute backend prefix (e.g. D:/file/learn/...).`,
    )
    requestAnimationFrame(() => {
      const input = pathInputRef.current
      if (input) {
        input.focus()
        const slashIdx = Math.max(
          composed.lastIndexOf('/'),
          composed.lastIndexOf('\\'),
        )
        const selectionEnd = slashIdx >= 0 ? slashIdx : composed.length
        input.setSelectionRange(0, selectionEnd)
      }
    })
  }

  const onPickFolder = async () => {
    setError(null)
    setInfo(null)
    setPickerHint(null)
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
    setInfo(null)
    try {
      const value = root.trim() ? root.trim() : null
      await update.mutateAsync({ group_workspace_root: value })
      if (value) saveRememberedPrefix(PICKER_SCOPE, value)
      setInfo('Saved system settings.')
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Network error')
    }
  }

  const onClear = async () => {
    setError(null)
    setInfo(null)
    try {
      await update.mutateAsync({ group_workspace_root: null })
      setRoot('')
      setPickerHint(null)
      setInfo('Group workspace root cleared.')
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
            <p className="mt-1 text-xs text-muted-foreground">
              Each group you create gets its own dedicated workspace under this
              directory. The backend creates a subfolder named after the group ID
              and stores group files there. Imported skills are also stored under
              <code className="px-1">{'<root>/skillshub/'}</code>.
            </p>

            <div className="mt-4 space-y-2">
              <Label htmlFor="ss-root">Local directory</Label>
              <div className="flex gap-2">
                <Input
                  id="ss-root"
                  ref={pathInputRef}
                  value={root}
                  onChange={(event) => onRootChange(event.target.value)}
                  placeholder="D:/workspaces/groups or /home/me/workspaces"
                />
                <Button
                  type="button"
                  variant="outline"
                  onClick={() => void onPickFolder()}
                >
                  Pick folder
                </Button>
              </div>
              <p className="text-[11px] text-muted-foreground">
                Browsers cannot return absolute filesystem paths for security.
                The picker fills in the folder name; we remember the absolute
                prefix you saved last time and prepend it on the next pick.
                Nothing is uploaded; the backend reads/writes this directory
                directly.
              </p>
              {pickerHint ? (
                <p className="text-[11px] text-muted-foreground">{pickerHint}</p>
              ) : null}
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
            {info ? <p className="mt-3 text-xs text-muted-foreground">{info}</p> : null}

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
