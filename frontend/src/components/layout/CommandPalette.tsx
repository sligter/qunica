import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import {
  BarChart3,
  Bot,
  Folder,
  Images,
  Languages,
  Monitor,
  Moon,
  Plug,
  Plus,
  ScrollText,
  Search,
  Server,
  SlidersHorizontal,
  Sparkles,
  Sun,
  type LucideIcon,
} from 'lucide-react'


import i18n, { normalizeLanguage, writeLanguageMirror } from '@/i18n'
import { useOverlayLinkState } from '@/components/layout/overlayRouting'
import {
  useSystemSettings,
  useUpdateSystemSettings,
} from '@/hooks/useSystemSettings'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import type { Appearance, Language } from '@/types/api'

/**
 * The shortcut hint as it should be *shown*: glyph on Apple platforms, spelled
 * out elsewhere. The listener itself accepts both modifiers on every platform.
 */
export function commandShortcutHint(): string {
  if (typeof navigator !== 'undefined' && /mac/i.test(navigator.platform)) {
    return '⌘K'
  }
  return 'Ctrl+K'
}

interface CommandItem {
  id: string
  label: string
  icon?: LucideIcon
  /** Extra search terms beyond the label (aliases, translations). */
  keywords?: string
  run: () => void
}

interface PaletteContextValue {
  /** Opens the palette. Safe to call from anywhere inside the authed shell. */
  open: () => void
}

const CommandPaletteContext = createContext<PaletteContextValue>({ open: () => {} })

/** Hook for sidebar buttons and other chrome that should summon the palette. */
export function useOpenCommandPalette(): () => void {
  return useContext(CommandPaletteContext).open
}

/**
 * Mounts once inside the authenticated shell: owns the open state, the global
 * Ctrl/Cmd+K chord, and the dialog itself. Consumers only ever see `open()`,
 * so no open-state plumbing threads through AppLayout props.
 *
 * The chord is global on purpose — it fires even while typing in a field, the
 * same way editors treat it. Nothing else in the app claims it.
 */
export function CommandPaletteProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false)

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        !event.altKey &&
        !event.shiftKey &&
        !event.isComposing &&
        event.key.toLowerCase() === 'k'
      ) {
        event.preventDefault()
        setOpen((current) => !current)
      }
    }
    document.addEventListener('keydown', onKeyDown)
    return () => document.removeEventListener('keydown', onKeyDown)
  }, [])

  const openPalette = useCallback(() => setOpen(true), [])

  return (
    <CommandPaletteContext.Provider value={{ open: openPalette }}>
      {children}
      <CommandPalette open={open} onOpenChange={setOpen} />
    </CommandPaletteContext.Provider>
  )
}

function CommandPalette({
  open,
  onOpenChange,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation([
    'navigation',
    'settings',
    'agents',
    'providers',
    'mcp',
    'skills',
    'workspaces',
    'assistant',
    'common',
  ])
  const close = useCallback(() => onOpenChange(false), [onOpenChange])
  const navigate = useNavigate()
  // Carries the conversation underneath so jumping from the stage opens the
  // area as an overlay over it, and jumping from inside an overlay stays there.
  const overlayState = useOverlayLinkState()
  const settingsQuery = useSystemSettings()
  const update = useUpdateSystemSettings()

  const [query, setQuery] = useState('')
  const [selected, setSelected] = useState(0)
  const [errorText, setErrorText] = useState<string | null>(null)
  const rowRefs = useRef<Array<HTMLButtonElement | null>>([])

  const appearance = settingsQuery.data?.appearance ?? 'system'
  const language = normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US'

  // Fresh palette every time it opens; stale filters or a half-scrolled
  // selection from last time are noise.
  useEffect(() => {
    if (open) {
      setQuery('')
      setSelected(0)
      setErrorText(null)
    }
  }, [open])

  const goTo = useCallback(
    (to: string) => {
      close()
      navigate(to, { state: overlayState })
    },
    [close, navigate, overlayState],
  )

  const changeAppearance = useCallback(
    async (next: Appearance) => {
      if (next === appearance) return
      setErrorText(null)
      try {
        await update.mutateAsync({ appearance: next })
        close()
      } catch (err) {
        setErrorText(err instanceof ApiError ? err.message : t('common:errors.unexpected'))
      }
    },
    [appearance, close, t, update],
  )

  const changeLanguage = useCallback(
    async (next: Language) => {
      if (next === language) return
      const previous = language
      setErrorText(null)
      await i18n.changeLanguage(next)
      try {
        await update.mutateAsync({ language: next })
        writeLanguageMirror(next)
        close()
      } catch (err) {
        // Roll the interface back so the label never lies about what was saved.
        await i18n.changeLanguage(previous)
        writeLanguageMirror(previous)
        setErrorText(err instanceof ApiError ? err.message : t('common:errors.unexpected'))
      }
    },
    [close, language, t, update],
  )

  const sections: Array<{
    key: 'navigate' | 'create' | 'preferences'
    label: string
    items: CommandItem[]
  }> = [
    {
      key: 'navigate',
      label: t('navigation:commandPalette.groups.navigate'),
      items: [
        { id: 'go-agents', label: t('navigation:agents'), icon: Bot, keywords: 'agent bot 智能体', run: () => goTo('/agents') },
        { id: 'go-providers', label: t('navigation:providers'), icon: Plug, keywords: 'provider llm model 服务商', run: () => goTo('/providers') },
        { id: 'go-mcp', label: t('navigation:mcpServers'), icon: Server, keywords: 'mcp server tool 服务', run: () => goTo('/mcp-servers') },
        { id: 'go-skills', label: t('navigation:skills'), icon: Sparkles, keywords: 'skill prompt 技能', run: () => goTo('/skills') },
        { id: 'go-workspaces', label: t('navigation:workspaces'), icon: Folder, keywords: 'workspace folder 工作区 目录', run: () => goTo('/workspaces') },
        { id: 'go-usage', label: t('navigation:usage'), icon: BarChart3, keywords: 'usage token cost 统计', run: () => goTo('/usage') },
        { id: 'go-settings-system', label: t('settings:tabs.system'), icon: SlidersHorizontal, keywords: 'settings system general 设置 系统', run: () => goTo('/settings/system') },
        { id: 'go-settings-media', label: t('settings:tabs.media'), icon: Images, keywords: 'settings media image 设置 媒体', run: () => goTo('/settings/media') },
        { id: 'go-settings-logs', label: t('settings:tabs.logs'), icon: ScrollText, keywords: 'settings logs runtime 设置 日志', run: () => goTo('/settings/logs') },
        { id: 'go-assistant-actions', label: t('assistant:actions.title'), icon: Sparkles, keywords: 'assistant actions 助手 动作', run: () => goTo('/settings/assistant-actions') },
      ],
    },
    {
      key: 'create',
      label: t('navigation:commandPalette.groups.create'),
      items: [
        { id: 'new-agent', label: t('agents:new'), icon: Plus, keywords: 'create agent 新建 智能体', run: () => goTo('/agents/new') },
        { id: 'new-provider', label: t('providers:new'), icon: Plus, keywords: 'create provider 新建 服务商', run: () => goTo('/providers/new') },
        { id: 'new-mcp', label: t('mcp:new'), icon: Plus, keywords: 'create mcp server 新建 服务', run: () => goTo('/mcp-servers/new') },
        { id: 'new-skill', label: t('skills:import'), icon: Plus, keywords: 'import skill 新建 技能', run: () => goTo('/skills/import') },
        { id: 'new-workspace', label: t('workspaces:new'), icon: Plus, keywords: 'create workspace folder 新建 工作区', run: () => goTo('/workspaces/new') },
      ],
    },
    {
      key: 'preferences',
      label: t('navigation:commandPalette.groups.preferences'),
      items: [
        { id: 'pref-light', label: t('settings:light'), icon: Sun, keywords: 'theme light 外观 亮色', run: () => void changeAppearance('light') },
        { id: 'pref-dark', label: t('settings:dark'), icon: Moon, keywords: 'theme dark 外观 暗色', run: () => void changeAppearance('dark') },
        { id: 'pref-system', label: t('settings:system'), icon: Monitor, keywords: 'theme system appearance 跟随系统', run: () => void changeAppearance('system') },
        { id: 'pref-zh', label: t('settings:chinese'), icon: Languages, keywords: 'language 中文 chinese', run: () => void changeLanguage('zh-CN') },
        { id: 'pref-en', label: t('settings:english'), icon: Languages, keywords: 'language english 英语', run: () => void changeLanguage('en-US') },
      ],
    },
  ]

  const q = query.trim().toLowerCase()
  const visibleSections = sections
    .map((section) => ({
      ...section,
      items: q
        ? section.items.filter((item) =>
            `${item.label} ${item.keywords ?? ''}`.toLowerCase().includes(q),
          )
        : section.items,
    }))
    .filter((section) => section.items.length > 0)
  const flat = visibleSections.flatMap((section) => section.items)

  // Filtering shrinks the list under the cursor; keep the selection inside it.
  useEffect(() => {
    setSelected((current) => Math.min(current, Math.max(0, flat.length - 1)))
  }, [flat.length])

  useEffect(() => {
    const row = rowRefs.current[selected]
    // jsdom ships no scrollIntoView; the guard keeps tests and old embeds honest.
    if (row && typeof row.scrollIntoView === 'function') {
      row.scrollIntoView({ block: 'nearest' })
    }
  }, [selected])

  const runSelected = () => {
    flat[selected]?.run()
  }

  const onContainerKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault()
      setSelected((current) => (flat.length ? (current + 1) % flat.length : 0))
    } else if (event.key === 'ArrowUp') {
      event.preventDefault()
      setSelected((current) =>
        flat.length ? (current - 1 + flat.length) % flat.length : 0,
      )
    } else if (event.key === 'Home') {
      event.preventDefault()
      setSelected(0)
    } else if (event.key === 'End') {
      event.preventDefault()
      setSelected(Math.max(0, flat.length - 1))
    } else if (event.key === 'Enter') {
      // Stop Radix's typeahead and any focused-button activation double-run;
      // the palette's own selection decides what Enter means.
      event.preventDefault()
      runSelected()
    }
  }

  let rowIndex = -1

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="top-[14%] max-w-xl translate-y-0 gap-0 p-2">
        <DialogHeader className="sr-only space-y-0">
          <DialogTitle>{t('navigation:commandPalette.title')}</DialogTitle>
          <DialogDescription>{t('navigation:commandPalette.description')}</DialogDescription>
        </DialogHeader>
        <div onKeyDown={onContainerKeyDown}>
          <div className="relative">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
            <Input
              autoFocus
              value={query}
              onChange={(event) => {
                setQuery(event.target.value)
                setSelected(0)
              }}
              placeholder={t('navigation:commandPalette.placeholder')}
              aria-label={t('navigation:commandPalette.placeholder')}
              className="h-11 rounded-lg border-0 bg-transparent pl-9 text-sm shadow-none focus-visible:ring-0"
            />
          </div>
          {errorText ? (
            <p role="alert" className="px-3 pb-1 text-xs text-destructive">
              {errorText}
            </p>
          ) : null}
          <div
            role="listbox"
            aria-label={t('navigation:commandPalette.placeholder')}
            className="max-h-[22rem] overflow-y-auto overscroll-contain p-1 pt-0"
          >
            {visibleSections.map((section) => (
              <div key={section.key}>
                <p className="px-2.5 pb-1 pt-2 text-2xs font-medium uppercase tracking-wider text-muted-foreground">
                  {section.label}
                </p>
                {section.items.map((item) => {
                  rowIndex += 1
                  const index = rowIndex
                  const Icon = item.icon
                  return (
                    <button
                      key={item.id}
                      ref={(node) => {
                        rowRefs.current[index] = node
                      }}
                      type="button"
                      role="option"
                      aria-selected={index === selected}
                      onMouseEnter={() => setSelected(index)}
                      onClick={item.run}
                      className={cn(
                        'flex h-9 w-full items-center gap-2.5 rounded-lg px-2.5 text-left text-sm outline-none transition-colors',
                        index === selected
                          ? 'bg-accent font-medium text-accent-foreground'
                          : 'text-foreground hover:bg-card-hover',
                        'focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring',
                      )}
                    >
                      {Icon ? <Icon className="h-4 w-4 shrink-0 text-muted-foreground" /> : null}
                      <span className="min-w-0 flex-1 truncate">{item.label}</span>
                    </button>
                  )
                })}
              </div>
            ))}
            {flat.length === 0 ? (
              <p className="px-3 py-6 text-center text-sm text-muted-foreground">
                {t('navigation:commandPalette.empty')}
              </p>
            ) : null}
          </div>
        </div>
      </DialogContent>
    </Dialog>
  )
}
