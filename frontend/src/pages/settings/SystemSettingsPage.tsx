import { useEffect, useRef, useState } from 'react'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import {
  useSystemSettings,
  useUpdateSystemSettings,
} from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/http'
import type { TavilySearchDepth } from '@/types/api'
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
  const [tavilyApiKey, setTavilyApiKey] = useState('')
  const [tavilySearchUrl, setTavilySearchUrl] = useState('')
  const [tavilyMaxResults, setTavilyMaxResults] = useState(5)
  const [tavilySearchDepth, setTavilySearchDepth] = useState<TavilySearchDepth>('basic')
  const [tavilyIncludeAnswer, setTavilyIncludeAnswer] = useState(true)
  const [tavilyIncludeRawContent, setTavilyIncludeRawContent] = useState(false)
  const [clearTavilyKey, setClearTavilyKey] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (settings.data) {
      setRoot(settings.data.group_workspace_root ?? '')
      setTavilyApiKey('')
      setTavilySearchUrl(settings.data.tavily_search_url ?? 'https://api.tavily.com/search')
      setTavilyMaxResults(settings.data.tavily_max_results ?? 5)
      setTavilySearchDepth(settings.data.tavily_search_depth ?? 'basic')
      setTavilyIncludeAnswer(settings.data.tavily_include_answer ?? true)
      setTavilyIncludeRawContent(settings.data.tavily_include_raw_content ?? false)
      setClearTavilyKey(false)
    }
  }, [settings.data])

  const onRootChange = (next: string) => {
    setRoot(next)
    saveRememberedPrefix(PICKER_SCOPE, next)
  }

  const applyPick = (folderName: string, absolutePath?: string) => {
    if (!folderName) return
    const remembered = readRememberedPrefix(PICKER_SCOPE)
    const composed = absolutePath ?? composePickedPath(root, folderName, remembered)
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
      applyPick(result.name, result.path)
      return
    }
    if (result.kind === 'cancelled') {
      return
    }
    if (result.kind === 'error') {
      setError(result.message)
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
      const nextKey = tavilyApiKey.trim()
      await update.mutateAsync({
        group_workspace_root: value,
        web_search_provider: 'tavily',
        tavily_api_key: clearTavilyKey ? null : nextKey || undefined,
        tavily_search_url: tavilySearchUrl.trim() || null,
        tavily_max_results: tavilyMaxResults,
        tavily_search_depth: tavilySearchDepth,
        tavily_include_answer: tavilyIncludeAnswer,
        tavily_include_raw_content: tavilyIncludeRawContent,
      })
      setTavilyApiKey('')
      setClearTavilyKey(false)
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

          <section className="rounded-lg border border-border bg-card p-5">
            <div className="flex items-start justify-between gap-4">
              <div>
                <h2 className="text-sm font-semibold">WebSearch provider</h2>
                <p className="mt-1 text-sm text-muted-foreground">
                  Bind the built-in WebSearch tool to Tavily for live web results.
                </p>
              </div>
              <span className="rounded-full bg-muted px-2.5 py-1 text-xs font-medium text-muted-foreground">
                Tavily
              </span>
            </div>

            <div className="mt-4 grid gap-4">
              <div className="space-y-2">
                <Label htmlFor="ss-tavily-key">API key</Label>
                <Input
                  id="ss-tavily-key"
                  type="password"
                  value={tavilyApiKey}
                  onChange={(event) => {
                    setTavilyApiKey(event.target.value)
                    setClearTavilyKey(false)
                  }}
                  placeholder={settings.data?.tavily_api_key_configured ? 'Configured; enter a new key to replace' : 'tvly-...'}
                />
                <div className="flex items-center justify-between gap-3 text-xs text-muted-foreground">
                  <span>{settings.data?.tavily_api_key_configured ? 'API key is configured.' : 'No Tavily API key saved.'}</span>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="h-7 px-2 text-xs"
                    onClick={() => {
                      setTavilyApiKey('')
                      setClearTavilyKey(true)
                    }}
                    disabled={!settings.data?.tavily_api_key_configured || update.isPending}
                  >
                    Clear key
                  </Button>
                </div>
                {clearTavilyKey ? <p className="text-xs text-amber-700">The saved API key will be cleared on save.</p> : null}
              </div>

              <div className="space-y-2">
                <Label htmlFor="ss-tavily-url">Service URL</Label>
                <Input
                  id="ss-tavily-url"
                  value={tavilySearchUrl}
                  onChange={(event) => setTavilySearchUrl(event.target.value)}
                  placeholder="https://api.tavily.com/search"
                />
              </div>

              <div className="grid gap-4 sm:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="ss-tavily-max-results">Max results</Label>
                  <Input
                    id="ss-tavily-max-results"
                    type="number"
                    min={1}
                    max={20}
                    value={tavilyMaxResults}
                    onChange={(event) => setTavilyMaxResults(Number(event.target.value))}
                  />
                </div>
                <div className="space-y-2">
                  <Label htmlFor="ss-tavily-depth">Search depth</Label>
                  <select
                    id="ss-tavily-depth"
                    className="h-10 w-full rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    value={tavilySearchDepth}
                    onChange={(event) => setTavilySearchDepth(event.target.value as TavilySearchDepth)}
                  >
                    <option value="basic">Basic</option>
                    <option value="advanced">Advanced</option>
                  </select>
                </div>
              </div>

              <div className="grid gap-3 sm:grid-cols-2">
                <label className="flex items-center gap-2 text-sm text-foreground">
                  <input
                    type="checkbox"
                    checked={tavilyIncludeAnswer}
                    onChange={(event) => setTavilyIncludeAnswer(event.target.checked)}
                  />
                  Include answer
                </label>
                <label className="flex items-center gap-2 text-sm text-foreground">
                  <input
                    type="checkbox"
                    checked={tavilyIncludeRawContent}
                    onChange={(event) => setTavilyIncludeRawContent(event.target.checked)}
                  />
                  Include raw content
                </label>
              </div>
            </div>

            <div className="mt-4 flex items-center gap-2">
              <Button onClick={onSave} disabled={update.isPending}>
                {update.isPending ? 'Saving…' : 'Save WebSearch'}
              </Button>
            </div>
          </section>
        </div>
      </main>
    </div>
  )
}
