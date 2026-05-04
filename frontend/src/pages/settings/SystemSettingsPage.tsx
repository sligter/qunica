import { useEffect, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  useSystemSettings,
  useUpdateSystemSettings,
} from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/api'

export function SystemSettingsPage() {
  const settings = useSystemSettings()
  const update = useUpdateSystemSettings()
  const [root, setRoot] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [info, setInfo] = useState<string | null>(null)

  useEffect(() => {
    if (settings.data) {
      setRoot(settings.data.group_workspace_root ?? '')
    }
  }, [settings.data])

  const onSave = async () => {
    setError(null)
    setInfo(null)
    try {
      await update.mutateAsync({
        group_workspace_root: root.trim() ? root.trim() : null,
      })
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
              <Input
                id="ss-root"
                value={root}
                onChange={(event) => setRoot(event.target.value)}
                placeholder="D:/workspaces/groups or /home/me/workspaces"
              />
              <p className="text-[11px] text-muted-foreground">
                Enter an absolute path on the machine running the backend.
                Nothing is uploaded; the backend reads/writes this directory
                directly.
              </p>
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
