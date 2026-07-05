import { useEffect, useRef, useState } from 'react'

import { DetailShell } from '@/components/layout/DetailShell'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { Switch } from '@/components/ui/switch'
import {
  useSystemSettings,
  useUpdateSystemSettings,
} from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/api-v2/client'
import type {
  Appearance,
  SystemSettingsUpdate,
  TavilySearchDepth,
} from '@/types/api'
import {
  composePickedPath,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
  type FolderPickResult,
} from '@/lib/folderPicker'

const PICKER_SCOPE = 'group-workspace-root'
const APPEARANCE_OPTIONS: Array<{ value: Appearance; label: string }> = [
  { value: 'light', label: 'Light' },
  { value: 'dark', label: 'Dark' },
  { value: 'system', label: 'System' },
]

export function SystemSettingsPage() {
  const settings = useSystemSettings()
  const update = useUpdateSystemSettings()
  const fallbackInputRef = useRef<HTMLInputElement | null>(null)
  const pathInputRef = useRef<HTMLInputElement | null>(null)
  const [appearance, setAppearance] = useState<Appearance>('system')
  const [root, setRoot] = useState('')
  const [tavilyApiKey, setTavilyApiKey] = useState('')
  const [tavilySearchUrl, setTavilySearchUrl] = useState('')
  const [tavilyMaxResults, setTavilyMaxResults] = useState(5)
  const [tavilySearchDepth, setTavilySearchDepth] = useState<TavilySearchDepth>('basic')
  const [tavilyIncludeAnswer, setTavilyIncludeAnswer] = useState(true)
  const [tavilyIncludeRawContent, setTavilyIncludeRawContent] = useState(false)
  const [clearTavilyKey, setClearTavilyKey] = useState(false)
  const [rootError, setRootError] = useState<string | null>(null)
  const [tavilyError, setTavilyError] = useState<string | null>(null)
  const [appearanceError, setAppearanceError] = useState<string | null>(null)

  // Sync each field from its own server value so saving one section does not
  // wipe unsaved edits in another (instant appearance saves refresh settings.data).
  const loaded = settings.data !== undefined
  const serverAppearance = settings.data?.appearance
  const serverRoot = settings.data?.group_workspace_root ?? ''
  const serverTavilyUrl = settings.data?.tavily_search_url ?? 'https://api.tavily.com/search'
  const serverTavilyMaxResults = settings.data?.tavily_max_results ?? 5
  const serverTavilyDepth = settings.data?.tavily_search_depth ?? 'basic'
  const serverTavilyIncludeAnswer = settings.data?.tavily_include_answer ?? true
  const serverTavilyIncludeRawContent = settings.data?.tavily_include_raw_content ?? false

  useEffect(() => {
    if (serverAppearance !== undefined) setAppearance(serverAppearance)
  }, [serverAppearance])
  useEffect(() => {
    if (loaded) setRoot(serverRoot)
  }, [loaded, serverRoot])
  useEffect(() => {
    if (loaded) setTavilySearchUrl(serverTavilyUrl)
  }, [loaded, serverTavilyUrl])
  useEffect(() => {
    if (loaded) setTavilyMaxResults(serverTavilyMaxResults)
  }, [loaded, serverTavilyMaxResults])
  useEffect(() => {
    if (loaded) setTavilySearchDepth(serverTavilyDepth)
  }, [loaded, serverTavilyDepth])
  useEffect(() => {
    if (loaded) setTavilyIncludeAnswer(serverTavilyIncludeAnswer)
  }, [loaded, serverTavilyIncludeAnswer])
  useEffect(() => {
    if (loaded) setTavilyIncludeRawContent(serverTavilyIncludeRawContent)
  }, [loaded, serverTavilyIncludeRawContent])

  const errorMessage = (err: unknown, fallback: string): string =>
    err instanceof ApiError ? err.message : fallback

  const onAppearanceChange = async (next: Appearance) => {
    if (next === appearance || update.isPending) return
    const previous = appearance
    setAppearance(next)
    setAppearanceError(null)
    try {
      await update.mutateAsync({ appearance: next })
    } catch (err) {
      setAppearance(previous)
      setAppearanceError(errorMessage(err, 'Appearance update failed'))
    }
  }

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
    setRootError(null)
    const result: FolderPickResult = await pickFolder()
    if (result.kind === 'native') {
      applyPick(result.name, result.path)
      return
    }
    if (result.kind === 'cancelled') {
      return
    }
    if (result.kind === 'error') {
      setRootError(result.message)
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

  const rootDirty = root.trim() !== serverRoot

  const onSaveRoot = async () => {
    setRootError(null)
    try {
      const value = root.trim() ? root.trim() : null
      await update.mutateAsync({ group_workspace_root: value })
      if (value) saveRememberedPrefix(PICKER_SCOPE, value)
    } catch (err) {
      setRootError(errorMessage(err, 'Network error'))
    }
  }

  const onClearRoot = async () => {
    setRootError(null)
    try {
      await update.mutateAsync({ group_workspace_root: null })
      setRoot('')
    } catch (err) {
      setRootError(errorMessage(err, 'Network error'))
    }
  }

  // Text-like Tavily fields save via the section Save button; switches and the
  // depth select save instantly (with revert on error) below.
  const tavilyDirty =
    tavilyApiKey.trim().length > 0 ||
    clearTavilyKey ||
    tavilySearchUrl.trim() !== serverTavilyUrl ||
    tavilyMaxResults !== serverTavilyMaxResults

  const onSaveTavily = async () => {
    setTavilyError(null)
    try {
      const nextKey = tavilyApiKey.trim()
      await update.mutateAsync({
        web_search_provider: 'tavily',
        tavily_api_key: clearTavilyKey ? null : nextKey || undefined,
        tavily_search_url: tavilySearchUrl.trim() || null,
        tavily_max_results: tavilyMaxResults,
      })
      setTavilyApiKey('')
      setClearTavilyKey(false)
    } catch (err) {
      setTavilyError(errorMessage(err, 'Network error'))
    }
  }

  const saveTavilyInstant = async (patch: SystemSettingsUpdate, revert: () => void) => {
    setTavilyError(null)
    try {
      await update.mutateAsync({ web_search_provider: 'tavily', ...patch })
    } catch (err) {
      revert()
      setTavilyError(errorMessage(err, 'Network error'))
    }
  }

  const onTavilyDepthChange = (value: string) => {
    if (value !== 'basic' && value !== 'advanced') return
    const previous = tavilySearchDepth
    setTavilySearchDepth(value)
    void saveTavilyInstant({ tavily_search_depth: value }, () =>
      setTavilySearchDepth(previous),
    )
  }

  const onTavilyIncludeAnswerChange = (next: boolean) => {
    const previous = tavilyIncludeAnswer
    setTavilyIncludeAnswer(next)
    void saveTavilyInstant({ tavily_include_answer: next }, () =>
      setTavilyIncludeAnswer(previous),
    )
  }

  const onTavilyIncludeRawContentChange = (next: boolean) => {
    const previous = tavilyIncludeRawContent
    setTavilyIncludeRawContent(next)
    void saveTavilyInstant({ tavily_include_raw_content: next }, () =>
      setTavilyIncludeRawContent(previous),
    )
  }

  return (
    <DetailShell
      title="System settings"
      subtitle="Account-level preferences and integrations."
    >
      <div className="space-y-10">
        <SettingsSection title="Appearance">
          <SettingsRow
            label="Theme"
            description="Choose the app theme for this account. Saved instantly."
          >
            <div
              className="inline-flex rounded-md border border-border bg-background p-1"
              role="radiogroup"
              aria-label="Appearance"
            >
              {APPEARANCE_OPTIONS.map((option) => (
                <Button
                  key={option.value}
                  type="button"
                  variant={appearance === option.value ? 'default' : 'ghost'}
                  size="sm"
                  className="min-w-20"
                  role="radio"
                  aria-checked={appearance === option.value}
                  disabled={update.isPending || settings.isLoading}
                  onClick={() => void onAppearanceChange(option.value)}
                >
                  {option.label}
                </Button>
              ))}
            </div>
          </SettingsRow>
          {appearanceError ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {appearanceError}
            </p>
          ) : null}
        </SettingsSection>

        <SettingsSection
          title="Group workspace root"
          aside={
            <Button
              size="sm"
              onClick={() => void onSaveRoot()}
              disabled={!rootDirty || update.isPending}
            >
              {update.isPending ? 'Saving…' : 'Save'}
            </Button>
          }
        >
          <SettingsRow
            label="Local directory"
            description="Group workspaces are created under this folder on the backend host."
            htmlFor="ss-root"
            stacked
          >
            <div className="flex gap-2">
              <Input
                id="ss-root"
                ref={pathInputRef}
                value={root}
                onChange={(event) => onRootChange(event.target.value)}
                placeholder="D:/workspaces/groups"
                className="max-w-xl"
              />
              <Button
                type="button"
                variant="outline"
                onClick={() => void onPickFolder()}
              >
                Pick folder
              </Button>
              <Button
                type="button"
                variant="ghost"
                onClick={() => void onClearRoot()}
                disabled={update.isPending || (!root.trim() && !serverRoot)}
              >
                Clear
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
            {rootError ? (
              <p className="text-sm text-destructive" role="alert">
                {rootError}
              </p>
            ) : null}
          </SettingsRow>
        </SettingsSection>

        <SettingsSection
          title="WebSearch (Tavily)"
          description="Bind the built-in WebSearch tool to Tavily for live web results."
          aside={
            <Button
              size="sm"
              onClick={() => void onSaveTavily()}
              disabled={!tavilyDirty || update.isPending}
            >
              {update.isPending ? 'Saving…' : 'Save'}
            </Button>
          }
        >
          <SettingsRow
            label="API key"
            description={
              clearTavilyKey
                ? 'The saved API key will be cleared on save.'
                : settings.data?.tavily_api_key_configured
                  ? 'API key is configured.'
                  : 'No Tavily API key saved.'
            }
            htmlFor="ss-tavily-key"
          >
            <Input
              id="ss-tavily-key"
              type="password"
              value={tavilyApiKey}
              onChange={(event) => {
                setTavilyApiKey(event.target.value)
                setClearTavilyKey(false)
              }}
              placeholder={
                settings.data?.tavily_api_key_configured
                  ? 'Configured; enter a new key to replace'
                  : 'tvly-...'
              }
              className="w-72"
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              onClick={() => {
                setTavilyApiKey('')
                setClearTavilyKey(true)
              }}
              disabled={!settings.data?.tavily_api_key_configured || update.isPending}
            >
              Clear key
            </Button>
          </SettingsRow>

          <SettingsRow
            label="Service URL"
            description="Endpoint used for Tavily search requests."
            htmlFor="ss-tavily-url"
          >
            <Input
              id="ss-tavily-url"
              value={tavilySearchUrl}
              onChange={(event) => setTavilySearchUrl(event.target.value)}
              placeholder="https://api.tavily.com/search"
              className="w-96"
            />
          </SettingsRow>

          <SettingsRow
            label="Max results"
            description="Number of search results per query (1-20)."
            htmlFor="ss-tavily-max-results"
          >
            <Input
              id="ss-tavily-max-results"
              type="number"
              min={1}
              max={20}
              value={tavilyMaxResults}
              onChange={(event) => setTavilyMaxResults(Number(event.target.value))}
              className="w-24"
            />
          </SettingsRow>

          <SettingsRow
            label="Search depth"
            description="Advanced depth returns richer results but is slower. Saved instantly."
            htmlFor="ss-tavily-depth"
          >
            <select
              id="ss-tavily-depth"
              className="h-9 w-40 rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={tavilySearchDepth}
              disabled={update.isPending}
              onChange={(event) => onTavilyDepthChange(event.target.value)}
            >
              <option value="basic">Basic</option>
              <option value="advanced">Advanced</option>
            </select>
          </SettingsRow>

          <SettingsRow
            label="Include answer"
            description="Ask Tavily to synthesize a short answer with the results. Saved instantly."
          >
            <Switch
              checked={tavilyIncludeAnswer}
              onCheckedChange={onTavilyIncludeAnswerChange}
              disabled={update.isPending}
              aria-label="Include answer"
            />
          </SettingsRow>

          <SettingsRow
            label="Include raw content"
            description="Attach raw page content to each result. Saved instantly."
          >
            <Switch
              checked={tavilyIncludeRawContent}
              onCheckedChange={onTavilyIncludeRawContentChange}
              disabled={update.isPending}
              aria-label="Include raw content"
            />
          </SettingsRow>

          {tavilyError ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {tavilyError}
            </p>
          ) : null}
        </SettingsSection>
      </div>
    </DetailShell>
  )
}
