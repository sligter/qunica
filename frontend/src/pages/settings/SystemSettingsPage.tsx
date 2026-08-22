import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

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
import { notificationsSupported, requestNotificationPermission, showNotification } from '@/lib/notifications'
import {
  readReplyNotificationsEnabled,
  writeReplyNotificationsEnabled,
} from '@/lib/replyNotifications'
import { writeLanguageMirror } from '@/i18n'
import type {
  Appearance,
  Language,
  ShellPreference,
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
const APPEARANCE_OPTIONS: Appearance[] = ['light', 'dark', 'system']
const LANGUAGE_OPTIONS: Language[] = ['zh-CN', 'en-US']
const SHELL_OPTIONS: ShellPreference[] = ['auto', 'powershell', 'bash', 'cmd']

export function SystemSettingsPage() {
  const { t, i18n } = useTranslation('settings')
  const settings = useSystemSettings()
  const update = useUpdateSystemSettings()
  const fallbackInputRef = useRef<HTMLInputElement | null>(null)
  const pathInputRef = useRef<HTMLInputElement | null>(null)
  const [appearance, setAppearance] = useState<Appearance>('system')
  const [language, setLanguage] = useState<Language>('en-US')
  const [assistantEnabled, setAssistantEnabled] = useState(true)
  const [replyNotifications, setReplyNotifications] = useState(readReplyNotificationsEnabled)
  const [notificationError, setNotificationError] = useState<string | null>(null)
  const [notificationTested, setNotificationTested] = useState(false)
  const [root, setRoot] = useState('')
  const [shellPreference, setShellPreference] = useState<ShellPreference>('auto')
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
  const [languageError, setLanguageError] = useState<string | null>(null)
  const [assistantError, setAssistantError] = useState<string | null>(null)
  const [shellError, setShellError] = useState<string | null>(null)

  // Sync each field from its own server value so saving one section does not
  // wipe unsaved edits in another (instant appearance saves refresh settings.data).
  const loaded = settings.data !== undefined
  const serverAppearance = settings.data?.appearance
  const serverLanguage = settings.data?.language
  const serverAssistantEnabled = settings.data?.assistant_enabled
  const serverRoot = settings.data?.group_workspace_root ?? ''
  const serverShellPreference = settings.data?.shell_preference
  const serverTavilyUrl = settings.data?.tavily_search_url ?? 'https://api.tavily.com/search'
  const serverTavilyMaxResults = settings.data?.tavily_max_results ?? 5
  const serverTavilyDepth = settings.data?.tavily_search_depth ?? 'basic'
  const serverTavilyIncludeAnswer = settings.data?.tavily_include_answer ?? true
  const serverTavilyIncludeRawContent = settings.data?.tavily_include_raw_content ?? false

  useEffect(() => {
    if (serverAppearance !== undefined) setAppearance(serverAppearance)
  }, [serverAppearance])
  useEffect(() => {
    if (serverLanguage !== undefined) setLanguage(serverLanguage)
  }, [serverLanguage])
  useEffect(() => {
    if (serverAssistantEnabled !== undefined) setAssistantEnabled(serverAssistantEnabled)
  }, [serverAssistantEnabled])
  useEffect(() => {
    if (loaded) setRoot(serverRoot)
  }, [loaded, serverRoot])
  useEffect(() => {
    if (serverShellPreference !== undefined) setShellPreference(serverShellPreference)
  }, [serverShellPreference])
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
  useEffect(() => {
    document.title = t('title')
  }, [i18n.resolvedLanguage, t])

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
      setAppearanceError(errorMessage(err, t('errors.appearance')))
    }
  }

  const onLanguageChange = async (next: Language) => {
    if (next === language || update.isPending) return
    const previous = language
    setLanguage(next)
    setLanguageError(null)
    await i18n.changeLanguage(next)
    try {
      await update.mutateAsync({ language: next })
      writeLanguageMirror(next)
    } catch (err) {
      setLanguage(previous)
      await i18n.changeLanguage(previous)
      writeLanguageMirror(previous)
      setLanguageError(errorMessage(err, t('errors.language')))
    }
  }

  const onAssistantEnabledChange = async (next: boolean) => {
    if (next === assistantEnabled || update.isPending) return
    const previous = assistantEnabled
    setAssistantEnabled(next)
    setAssistantError(null)
    try {
      await update.mutateAsync({ assistant_enabled: next })
    } catch (err) {
      setAssistantEnabled(previous)
      setAssistantError(errorMessage(err, t('errors.assistant')))
    }
  }

  const onRootChange = (next: string) => {
    setRoot(next)
    saveRememberedPrefix(PICKER_SCOPE, next)
  }

  /**
   * Per device, not per account: it pairs with an OS permission granted on this
   * machine, so mirroring it to every device the account signs in from would be
   * the wrong promise. Turning it on asks the browser for that permission now,
   * rather than losing the first notification to an unanswered prompt.
   */
  const onReplyNotificationsChange = async (next: boolean) => {
    setNotificationError(null)
    setNotificationTested(false)
    if (!next) {
      setReplyNotifications(false)
      writeReplyNotificationsEnabled(false)
      return
    }
    if (!notificationsSupported()) {
      setNotificationError(t('notifications.unsupported'))
      return
    }
    const permission = await requestNotificationPermission()
    if (permission !== 'granted') {
      setNotificationError(t('notifications.permissionDenied'))
      return
    }
    setReplyNotifications(true)
    writeReplyNotificationsEnabled(true)
  }

  /**
   * Prove the delivery path end to end.
   *
   * A notification that never arrives leaves nothing behind to look at — no
   * error, no log line the user would think to open — so the one place that
   * claims the feature exists is also where they can make it fire on demand and
   * read back why it did not.
   */
  const onTestNotification = async () => {
    setNotificationError(null)
    setNotificationTested(false)
    const result = await showNotification(
      t('notifications.testTitle'),
      t('notifications.testBody'),
    )
    if (result.ok) {
      setNotificationTested(true)
      return
    }
    setNotificationError(t('notifications.testFailed', { message: result.error }))
  }

  const onShellPreferenceChange = async (value: string) => {
    const next = SHELL_OPTIONS.find((option) => option === value)
    if (next === undefined || next === shellPreference || update.isPending) return
    const previous = shellPreference
    setShellPreference(next)
    setShellError(null)
    try {
      await update.mutateAsync({ shell_preference: next })
    } catch (err) {
      setShellPreference(previous)
      setShellError(errorMessage(err, t('errors.network')))
    }
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
      setRootError(errorMessage(err, t('errors.network')))
    }
  }

  const onClearRoot = async () => {
    setRootError(null)
    try {
      await update.mutateAsync({ group_workspace_root: null })
      setRoot('')
    } catch (err) {
      setRootError(errorMessage(err, t('errors.network')))
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
      setTavilyError(errorMessage(err, t('errors.network')))
    }
  }

  const saveTavilyInstant = async (patch: SystemSettingsUpdate, revert: () => void) => {
    setTavilyError(null)
    try {
      await update.mutateAsync({ web_search_provider: 'tavily', ...patch })
    } catch (err) {
      revert()
      setTavilyError(errorMessage(err, t('errors.network')))
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
      title={t('title')}
      subtitle={t('subtitle')}
    >
      <div className="space-y-10">
        <SettingsSection title={t('appearance')}>
          <SettingsRow
            label={t('theme')}
            description={t('themeDescription')}
          >
            <div
              className="inline-flex rounded-md border border-border bg-background p-1"
              role="radiogroup"
              aria-label={t('appearance')}
            >
              {APPEARANCE_OPTIONS.map((option) => (
                <Button
                  key={option}
                  type="button"
                  variant={appearance === option ? 'default' : 'ghost'}
                  size="sm"
                  className="min-w-20"
                  role="radio"
                  aria-checked={appearance === option}
                  disabled={update.isPending || settings.isLoading}
                  onClick={() => void onAppearanceChange(option)}
                >
                  {t(option)}
                </Button>
              ))}
            </div>
          </SettingsRow>
          {appearanceError ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {appearanceError}
            </p>
          ) : null}
          <SettingsRow
            label={t('language')}
            description={t('languageDescription')}
          >
            <div
              className="inline-flex rounded-md border border-border bg-background p-1"
              role="radiogroup"
              aria-label={t('language')}
            >
              {LANGUAGE_OPTIONS.map((option) => (
                <Button
                  key={option}
                  type="button"
                  variant={language === option ? 'default' : 'ghost'}
                  size="sm"
                  className="min-w-20"
                  role="radio"
                  aria-checked={language === option}
                  disabled={update.isPending || settings.isLoading}
                  onClick={() => void onLanguageChange(option)}
                >
                  {t(option === 'zh-CN' ? 'chinese' : 'english')}
                </Button>
              ))}
            </div>
          </SettingsRow>
          {languageError ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {languageError}
            </p>
          ) : null}
        </SettingsSection>

        <SettingsSection title={t('assistant.title')}>
          <SettingsRow
            label={t('assistant.enabled')}
            description={t('assistant.enabledDescription')}
          >
            <Switch
              checked={assistantEnabled}
              disabled={settings.isLoading || update.isPending}
              onCheckedChange={(next) => void onAssistantEnabledChange(next)}
              aria-label={t('assistant.enabled')}
            />
          </SettingsRow>
          {assistantError ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {assistantError}
            </p>
          ) : null}
        </SettingsSection>

        <SettingsSection title={t('notifications.title')}>
          <SettingsRow
            label={t('notifications.replyFinished')}
            description={t('notifications.replyFinishedDescription')}
          >
            <Switch
              checked={replyNotifications}
              onCheckedChange={(next) => void onReplyNotificationsChange(next)}
              aria-label={t('notifications.replyFinished')}
            />
          </SettingsRow>
          <SettingsRow label={t('notifications.test')}>
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => void onTestNotification()}
            >
              {t('notifications.test')}
            </Button>
          </SettingsRow>
          {notificationTested ? (
            <p className="py-2 text-sm text-muted-foreground" role="status">
              {t('notifications.testSent')}
            </p>
          ) : null}
          {notificationError ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {notificationError}
            </p>
          ) : null}
        </SettingsSection>

        <SettingsSection title={t('shell.title')}>
          <SettingsRow
            label={t('shell.integratedShell')}
            description={t('shell.description')}
            htmlFor="ss-shell-preference"
          >
            <select
              id="ss-shell-preference"
              className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={shellPreference}
              disabled={settings.isLoading || update.isPending}
              onChange={(event) => void onShellPreferenceChange(event.target.value)}
            >
              {SHELL_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {t(`shell.options.${option}`)}
                </option>
              ))}
            </select>
          </SettingsRow>
          {shellError ? (
            <p className="py-2 text-sm text-destructive" role="alert">
              {shellError}
            </p>
          ) : null}
        </SettingsSection>

        <SettingsSection
          title={t('workspaceRoot.title')}
          aside={
            <Button
              size="sm"
              onClick={() => void onSaveRoot()}
              disabled={!rootDirty || update.isPending}
            >
              {update.isPending ? t('common:actions.saving') : t('common:actions.save')}
            </Button>
          }
        >
          <SettingsRow
            label={t('workspaceRoot.directory')}
            description={t('workspaceRoot.description')}
            htmlFor="ss-root"
            stacked
          >
            <div className="flex w-full gap-2">
              <Input
                id="ss-root"
                ref={pathInputRef}
                value={root}
                onChange={(event) => onRootChange(event.target.value)}
                placeholder={t('workspaceRoot.placeholder')}
                className="min-w-0 flex-1"
              />
              <Button
                type="button"
                variant="outline"
                onClick={() => void onPickFolder()}
              >
                {t('workspaceRoot.pickFolder')}
              </Button>
              <Button
                type="button"
                variant="ghost"
                onClick={() => void onClearRoot()}
                disabled={update.isPending || (!root.trim() && !serverRoot)}
              >
                {t('common:actions.clear')}
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
          title={t('tavily.title')}
          description={t('tavily.description')}
          aside={
            <Button
              size="sm"
              onClick={() => void onSaveTavily()}
              disabled={!tavilyDirty || update.isPending}
            >
              {update.isPending ? t('common:actions.saving') : t('common:actions.save')}
            </Button>
          }
        >
          <SettingsRow
            label={t('tavily.apiKey')}
            description={
              clearTavilyKey
                ? t('tavily.keyWillClear')
                : settings.data?.tavily_api_key_configured
                  ? t('tavily.keyConfigured')
                  : t('tavily.keyMissing')
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
                  ? t('tavily.configuredPlaceholder')
                  : 'tvly-...'
              }
              className="min-w-0 flex-1"
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
              {t('tavily.clearKey')}
            </Button>
          </SettingsRow>

          <SettingsRow
            label={t('tavily.serviceUrl')}
            description={t('tavily.serviceUrlDescription')}
            htmlFor="ss-tavily-url"
          >
            <Input
              id="ss-tavily-url"
              value={tavilySearchUrl}
              onChange={(event) => setTavilySearchUrl(event.target.value)}
              placeholder="https://api.tavily.com/search"
              className="w-full"
            />
          </SettingsRow>

          <SettingsRow
            label={t('tavily.maxResults')}
            description={t('tavily.maxResultsDescription')}
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
            label={t('tavily.searchDepth')}
            description={t('tavily.searchDepthDescription')}
            htmlFor="ss-tavily-depth"
          >
            <select
              id="ss-tavily-depth"
              className="h-9 w-full rounded-md border border-input bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              value={tavilySearchDepth}
              disabled={update.isPending}
              onChange={(event) => onTavilyDepthChange(event.target.value)}
            >
              <option value="basic">{t('tavily.basic')}</option>
              <option value="advanced">{t('tavily.advanced')}</option>
            </select>
          </SettingsRow>

          <SettingsRow
            label={t('tavily.includeAnswer')}
            description={t('tavily.includeAnswerDescription')}
          >
            <Switch
              checked={tavilyIncludeAnswer}
              onCheckedChange={onTavilyIncludeAnswerChange}
              disabled={update.isPending}
              aria-label={t('tavily.includeAnswer')}
            />
          </SettingsRow>

          <SettingsRow
            label={t('tavily.includeRawContent')}
            description={t('tavily.includeRawContentDescription')}
          >
            <Switch
              checked={tavilyIncludeRawContent}
              onCheckedChange={onTavilyIncludeRawContentChange}
              disabled={update.isPending}
              aria-label={t('tavily.includeRawContent')}
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
