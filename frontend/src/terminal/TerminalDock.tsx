import {
  ChevronDown,
  Maximize2,
  Minimize2,
  Pencil,
  Plus,
  RotateCw,
  ShieldAlert,
  SquareTerminal,
  X,
} from 'lucide-react'
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useRef,
  useState,
  type ComponentType,
  type KeyboardEvent,
} from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { cn } from '@/lib/utils'
import {
  useTerminalRuntime,
  type TerminalRuntimeTab,
} from '@/terminal/TerminalRuntimeProvider'
import { usePersistentPaneHeight } from '@/terminal/usePersistentPaneHeight'

export const FULL_ACCESS_WARNING_KEY = 'ag-swarmer:terminal-full-access-warning:v1'

// The dock ships with every route but starts collapsed and has no tabs until the
// user opens one, so the xterm runtime downloads with the first pane instead of
// at boot. `lazy` caches the module, so later tabs render synchronously and
// existing panes are never remounted.
const TerminalPane = lazy(() =>
  import('@/terminal/TerminalPane').then((m) => ({ default: m.TerminalPane })),
)

interface IconActionProps {
  label: string
  icon: ComponentType<{ className?: string }>
  onClick(): void
  disabled?: boolean
  active?: boolean
}

function IconAction({ label, icon: Icon, onClick, disabled, active }: IconActionProps) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon"
      className={cn(
        'h-8 w-8 shrink-0 rounded-sm text-muted-foreground hover:text-foreground',
        'focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1',
        active && 'bg-muted text-foreground',
      )}
      aria-label={label}
      title={label}
      disabled={disabled}
      onClick={onClick}
    >
      <Icon className="h-3.5 w-3.5" />
    </Button>
  )
}

function statusClass(status: TerminalRuntimeTab['status']): string {
  if (status === 'running') return 'bg-success'
  if (status === 'starting') return 'bg-warning-foreground animate-pulse'
  if (status === 'error') return 'bg-destructive'
  return 'bg-muted-foreground/60'
}

function readWarningDismissed(): boolean {
  try {
    return localStorage.getItem(FULL_ACCESS_WARNING_KEY) === 'dismissed'
  } catch {
    return false
  }
}

function rememberWarningDismissed(): void {
  try {
    localStorage.setItem(FULL_ACCESS_WARNING_KEY, 'dismissed')
  } catch {
    // A storage failure should not prevent terminal use.
  }
}

export function TerminalDock() {
  const { t } = useTranslation('chat')
  const runtime = useTerminalRuntime()
  const hostRef = useRef<HTMLElement>(null)
  const [availableHeight, setAvailableHeight] = useState(() => (
    typeof window === 'undefined' ? 800 : window.innerHeight
  ))
  const [renameTabId, setRenameTabId] = useState<string | null>(null)
  const [renameDraft, setRenameDraft] = useState('')
  const [warningDismissed, setWarningDismissed] = useState(readWarningDismissed)

  useEffect(() => {
    const parent = hostRef.current?.parentElement
    if (parent === null || parent === undefined) return
    const measure = () => {
      const next = Math.round(parent.getBoundingClientRect().height)
      if (next > 0) setAvailableHeight(next)
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(parent)
    return () => observer.disconnect()
  }, [])

  const paneHeight = usePersistentPaneHeight({
    availableHeight,
    persistedHeight: runtime.paneHeight,
    onPersist: runtime.setPaneHeight,
  })

  const activeTarget = runtime.activeConversation
  const chromeVisible = activeTarget !== null && runtime.isDockOpen
  const ready = activeTarget?.availability === 'ready'
  const runtimeChromeVisible = chromeVisible && ready
  const displayedHeight = runtime.isMaximized ? availableHeight : paneHeight.height
  const activeTab = runtime.activeTabs.find((tab) => tab.tabId === runtime.activeTabId) ?? null
  const separatorMin = Math.min(180, Math.max(0, availableHeight))
  const separatorMax = Math.max(
    separatorMin,
    runtime.isMaximized ? availableHeight : Math.round(availableHeight * 0.7),
  )
  const separatorNow = Math.min(separatorMax, Math.max(separatorMin, displayedHeight))

  const startRename = useCallback((tab: TerminalRuntimeTab | null) => {
    if (tab === null) return
    setRenameTabId(tab.tabId)
    setRenameDraft(tab.label)
  }, [])

  const commitRename = useCallback(() => {
    if (renameTabId === null) return
    const label = renameDraft.trim()
    if (label !== '') runtime.renameTab(renameTabId, label)
    setRenameTabId(null)
  }, [renameDraft, renameTabId, runtime])

  const onRenameKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter') {
      event.preventDefault()
      commitRename()
    } else if (event.key === 'Escape') {
      event.preventDefault()
      setRenameTabId(null)
    }
  }

  const dismissWarning = () => {
    rememberWarningDismissed()
    setWarningDismissed(true)
  }

  const unavailableMessage = (() => {
    switch (activeTarget?.availability) {
      case 'loading':
        return t('terminal.loading')
      case 'desktopRequired':
        return t('terminal.desktopRequired')
      case 'workspaceRequired':
        return t('terminal.workspaceRequired')
      case 'localWorkspaceRequired':
        return t('terminal.localWorkspaceRequired')
      case 'pathRequired':
        return t('terminal.pathRequired')
      default:
        return null
    }
  })()

  return (
    <section
      ref={hostRef}
      data-testid="terminal-dock-host"
      className="terminal-dock relative flex shrink-0 flex-col overflow-hidden border-t border-border bg-[var(--terminal-background)] text-[var(--terminal-foreground)]"
      style={{ height: chromeVisible ? displayedHeight : 0 }}
      aria-hidden={!chromeVisible}
    >
      {chromeVisible ? (
        <>
          <div
            role="separator"
            aria-orientation="horizontal"
            aria-label={t('terminal.resize')}
            aria-valuemin={separatorMin}
            aria-valuemax={separatorMax}
            aria-valuenow={separatorNow}
            tabIndex={0}
            className="terminal-resize-handle absolute inset-x-0 top-0 z-10 h-3 cursor-row-resize touch-none focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring"
            onDoubleClick={paneHeight.reset}
            {...paneHeight.separatorProps}
          />

          <div className="flex h-8 min-h-8 items-center gap-1 border-b border-[var(--terminal-border)] bg-[var(--terminal-chrome)] px-1.5">
            {ready ? (
              <div className="flex min-w-0 flex-1 items-stretch gap-0.5 overflow-x-auto" role="tablist">
                {runtime.activeTabs.map((tab) => {
                  const selected = tab.tabId === runtime.activeTabId
                  return (
                    <div
                      key={tab.tabId}
                      className={cn(
                        'group flex h-8 min-w-[7rem] max-w-52 items-center border-b-2 px-2 text-xs',
                        selected
                          ? 'border-primary bg-[var(--terminal-active-tab)] text-[var(--terminal-foreground)]'
                          : 'border-transparent text-[var(--terminal-inactive-tab)] hover:bg-[var(--terminal-active-tab)] hover:text-[var(--terminal-foreground)]',
                      )}
                    >
                      <span className={cn('mr-2 h-1.5 w-1.5 shrink-0 rounded-full', statusClass(tab.status))} />
                      {renameTabId === tab.tabId ? (
                        <input
                          autoFocus
                          aria-label={t('terminal.rename')}
                          className="h-6 min-w-0 flex-1 rounded-sm border border-ring bg-[var(--terminal-background)] px-1.5 text-xs text-[var(--terminal-foreground)] outline-none"
                          value={renameDraft}
                          onChange={(event) => setRenameDraft(event.target.value)}
                          onBlur={commitRename}
                          onKeyDown={onRenameKeyDown}
                        />
                      ) : (
                        <button
                          type="button"
                          role="tab"
                          aria-selected={selected}
                          className="min-w-0 flex-1 truncate text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                          title={tab.label}
                          onClick={() => runtime.selectTab(tab.tabId)}
                          onDoubleClick={() => startRename(tab)}
                        >
                          {tab.label}
                        </button>
                      )}
                      <button
                        type="button"
                        className="ml-1 grid h-6 w-6 shrink-0 place-items-center rounded-sm opacity-60 hover:bg-muted hover:opacity-100 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring group-hover:opacity-100"
                        aria-label={`${t('terminal.close')}: ${tab.label}`}
                        title={t('terminal.close')}
                        onClick={() => void runtime.closeTab(tab.tabId).catch(() => undefined)}
                      >
                        <X className="h-3 w-3" />
                      </button>
                    </div>
                  )
                })}
              </div>
            ) : (
              <div className="min-w-0 flex-1 px-2 text-xs font-medium text-[var(--terminal-inactive-tab)]">
                {t('terminal.title')}
              </div>
            )}

            <div className="flex shrink-0 items-center border-l border-[var(--terminal-border)] pl-1">
              {ready ? (
                <>
                  <IconAction
                    label={t('terminal.new')}
                    icon={Plus}
                    onClick={() => void runtime.createTab().catch(() => undefined)}
                  />
                  <IconAction
                    label={t('terminal.rename')}
                    icon={Pencil}
                    disabled={activeTab === null}
                    onClick={() => startRename(activeTab)}
                  />
                  <IconAction
                    label={t('terminal.close')}
                    icon={X}
                    disabled={activeTab === null}
                    onClick={() => activeTab && void runtime.closeTab(activeTab.tabId).catch(() => undefined)}
                  />
                  <IconAction
                    label={t('terminal.restart')}
                    icon={RotateCw}
                    disabled={activeTab === null}
                    onClick={() => activeTab && void runtime.restartTab(activeTab.tabId).catch(() => undefined)}
                  />
                </>
              ) : null}
              <IconAction
                label={runtime.isMaximized
                  ? t('terminal.restore')
                  : t('terminal.maximize')}
                icon={runtime.isMaximized ? Minimize2 : Maximize2}
                active={runtime.isMaximized}
                onClick={runtime.toggleMaximized}
              />
              <IconAction
                label={t('terminal.collapse')}
                icon={ChevronDown}
                onClick={() => void runtime.toggleDock().catch(() => undefined)}
              />
            </div>
          </div>

          {ready && !warningDismissed ? (
            <div className="flex min-h-10 items-center gap-2 border-b border-[var(--terminal-border)] bg-warning px-3 py-1.5 text-xs text-warning-foreground">
              <ShieldAlert className="h-4 w-4 shrink-0" aria-hidden="true" />
              <p className="min-w-0 flex-1">
                <strong className="mr-1 font-semibold">{t('terminal.fullAccessTitle')}</strong>
                {t('terminal.fullAccessBody')}
              </p>
              <Button type="button" variant="outline" size="sm" className="h-7 bg-transparent" onClick={dismissWarning}>
                {t('terminal.dismiss')}
              </Button>
            </div>
          ) : null}
        </>
      ) : null}

      <div className="relative min-h-0 flex-1" data-testid="terminal-pane-host">
        <Suspense fallback={null}>
          {runtime.allTabs.map((tab) => {
            const paneVisible = runtimeChromeVisible && tab.tabId === runtime.activeTabId
            return (
              <div
                key={tab.tabId}
                hidden={!paneVisible}
                aria-hidden={!paneVisible}
                className="absolute inset-0 p-2"
              >
                <TerminalPane tab={tab} />
              </div>
            )
          })}
        </Suspense>

        {chromeVisible && unavailableMessage !== null ? (
          <div className="absolute inset-0 grid place-items-center p-6 text-center">
            <div className="max-w-md">
              <SquareTerminal className="mx-auto mb-3 h-7 w-7 text-[var(--terminal-inactive-tab)]" aria-hidden="true" />
              <p className="text-sm text-[var(--terminal-inactive-tab)]">{unavailableMessage}</p>
            </div>
          </div>
        ) : runtimeChromeVisible && runtime.activeTabs.length === 0 ? (
          <div className="absolute inset-0 grid place-items-center p-6 text-center">
            <div>
              <SquareTerminal className="mx-auto mb-3 h-7 w-7 text-[var(--terminal-inactive-tab)]" aria-hidden="true" />
              <p className="mb-3 text-sm text-[var(--terminal-inactive-tab)]">
                {t('terminal.empty')}
              </p>
              <Button type="button" variant="outline" size="sm" onClick={() => void runtime.createTab().catch(() => undefined)}>
                <Plus className="h-3.5 w-3.5" />
                {t('terminal.new')}
              </Button>
            </div>
          </div>
        ) : null}

        {runtimeChromeVisible && activeTab?.status === 'starting' ? (
          <div className="pointer-events-none absolute right-3 top-2 rounded bg-[var(--terminal-chrome)] px-2 py-1 text-[11px] text-[var(--terminal-inactive-tab)]">
            {t('terminal.starting')}
          </div>
        ) : null}
        {runtimeChromeVisible && activeTab?.status === 'exited' ? (
          <div className="absolute inset-x-0 bottom-0 flex items-center justify-between border-t border-[var(--terminal-border)] bg-[var(--terminal-chrome)] px-3 py-2 text-xs">
            <span className="text-[var(--terminal-inactive-tab)]">
              {activeTab.exitCode === null
                ? t('terminal.exited')
                : t('terminal.exitCode', { code: activeTab.exitCode })}
            </span>
            <Button type="button" variant="outline" size="sm" className="h-7" onClick={() => void runtime.restartTab(activeTab.tabId).catch(() => undefined)}>
              <RotateCw className="h-3.5 w-3.5" />
              {t('terminal.restart')}
            </Button>
          </div>
        ) : null}
        {runtimeChromeVisible && activeTab?.status === 'error' ? (
          <div className="absolute inset-x-0 bottom-0 flex items-center justify-between gap-3 border-t border-destructive/40 bg-[var(--terminal-chrome)] px-3 py-2 text-xs">
            <span className="min-w-0 truncate text-destructive" title={activeTab.error?.message}>
              {t('terminal.spawnError', { message: activeTab.error?.message ?? '' })}
            </span>
            <Button type="button" variant="outline" size="sm" className="h-7 shrink-0" onClick={() => void runtime.restartTab(activeTab.tabId).catch(() => undefined)}>
              <RotateCw className="h-3.5 w-3.5" />
              {t('terminal.retry')}
            </Button>
          </div>
        ) : null}
      </div>
    </section>
  )
}
