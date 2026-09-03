import {
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import {
  ArrowLeft,
  ArrowRight,
  Bot,
  Check,
  CheckCircle2,
  FolderOpen,
  KeyRound,
  Loader2,
  MessagesSquare,
  Plus,
  Server,
  Sparkles,
} from 'lucide-react'
import { useTranslation } from 'react-i18next'
import { Outlet } from 'react-router-dom'

import { BrandMark } from '@/components/brand/BrandMark'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ServerFolderPicker } from '@/components/workspace/ServerFolderPicker'
import { useAssistant, useUpdateAssistant } from '@/hooks/useAssistant'
import { useProviders } from '@/hooks/useProviders'
import { useSystemSettings, useUpdateSystemSettings } from '@/hooks/useSystemSettings'
import { normalizeLanguage } from '@/i18n'
import {
  composePickedPath,
  normalizeWindowsPath,
  pickFolder,
  readRememberedPrefix,
  saveRememberedPrefix,
  type FolderPickResult,
} from '@/lib/folderPicker'
import { cn } from '@/lib/utils'
import type { LLMProviderRead } from '@/types/api'

const PICKER_SCOPE = 'group-workspace-root'
const CreateProviderForm = lazy(() =>
  import('@/components/providers/CreateProviderForm').then((module) => ({
    default: module.CreateProviderForm,
  })),
)

type Phase = 'workspace' | 'provider' | 'model' | 'ready'

export function FirstRunGate() {
  const { t } = useTranslation('onboarding')
  const settings = useSystemSettings()

  if (settings.isLoading) {
    return <SetupStatus title={t('loading')} />
  }
  if (settings.isError || !settings.data) {
    return (
      <SetupStatus
        title={t('loadError')}
        action={
          <Button variant="outline" onClick={() => void settings.refetch()}>
            {t('retry')}
          </Button>
        }
      />
    )
  }

  // Missing on older servers means "unsupported", not "first run". That
  // keeps a staggered desktop/backend update from trapping the user in a guide
  // the older API cannot complete.
  if (settings.data.onboarding_completed !== false) return <Outlet />

  return <FirstRunSetup initialRoot={settings.data.group_workspace_root ?? ''} />
}

function SetupStatus({ title, action }: { title: string; action?: ReactNode }) {
  return (
    <main className="relative flex h-full items-center justify-center overflow-hidden bg-background p-8">
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_72%_18%,color-mix(in_srgb,var(--color-primary)_12%,transparent),transparent_34%)]" />
      <div className="relative flex flex-col items-center text-center">
        <BrandMark animated className="h-16 w-16 shadow-lg" />
        <h1 className="mt-5 font-serif text-xl font-semibold tracking-tight">{title}</h1>
        {action ? <div className="mt-5">{action}</div> : <Loader2 className="mt-5 h-4 w-4 animate-spin text-primary" aria-hidden />}
      </div>
    </main>
  )
}

function FirstRunSetup({ initialRoot }: { initialRoot: string }) {
  const { t, i18n } = useTranslation('onboarding')
  const providers = useProviders()
  const assistant = useAssistant()
  const updateAssistant = useUpdateAssistant()
  const updateSettings = useUpdateSystemSettings()
  const [browsing, setBrowsing] = useState(false)
  const pathInputRef = useRef<HTMLInputElement | null>(null)
  const contentRef = useRef<HTMLDivElement | null>(null)

  const [phase, setPhase] = useState<Phase>(initialRoot ? 'provider' : 'workspace')
  const [root, setRoot] = useState(() => normalizeWindowsPath(initialRoot))
  const [selectedProvider, setSelectedProvider] = useState<LLMProviderRead | null>(null)
  const [model, setModel] = useState('')
  const [creatingProvider, setCreatingProvider] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const phaseIndex = phase === 'workspace' ? 0 : phase === 'provider' ? 1 : phase === 'model' ? 2 : 3
  const steps = [
    { icon: FolderOpen, title: t('progress.workspace'), detail: t('progress.workspaceDetail') },
    { icon: Server, title: t('progress.provider'), detail: t('progress.providerDetail') },
    { icon: Bot, title: t('progress.model'), detail: t('progress.modelDetail') },
  ]
  const modelOptions = useMemo(
    () => Array.from(new Set([
      selectedProvider?.default_model,
      ...(selectedProvider?.models ?? []).map((entry) => entry.id),
    ].filter((value): value is string => Boolean(value)))),
    [selectedProvider],
  )

  useEffect(() => {
    document.title = t('documentTitle')
  }, [i18n.resolvedLanguage, t])

  useEffect(() => {
    if (selectedProvider || !providers.data) return
    const current = providers.data.find((provider) => provider.id === assistant.data?.provider_id)
    if (!current) return
    setSelectedProvider(current)
    setModel(assistant.data?.model ?? current.default_model)
  }, [assistant.data, providers.data, selectedProvider])

  const go = (next: Phase) => {
    setError(null)
    setPhase(next)
    contentRef.current?.scrollTo?.({ top: 0 })
    requestAnimationFrame(() => {
      contentRef.current?.querySelector<HTMLElement>('[data-onboarding-title]')?.focus()
    })
  }

  const applyPick = (folderName: string, absolutePath?: string) => {
    if (!folderName) return
    const composed = absolutePath ?? composePickedPath(
      root,
      folderName,
      readRememberedPrefix(PICKER_SCOPE),
    )
    setRoot(composed)
    saveRememberedPrefix(PICKER_SCOPE, composed)
    requestAnimationFrame(() => pathInputRef.current?.focus())
  }

  const onPickFolder = async () => {
    setError(null)
    const result: FolderPickResult = await pickFolder()
    if (result.kind === 'native') return applyPick(result.name, result.path)
    if (result.kind === 'serverBrowse') return setBrowsing(true)
    if (result.kind === 'error') setError(t('workspace.pickerError'))
  }

  const saveWorkspace = async () => {
    const value = root.trim()
    if (!value) {
      setError(t('workspace.required'))
      pathInputRef.current?.focus()
      return
    }
    setError(null)
    try {
      const updated = await updateSettings.mutateAsync({ group_workspace_root: value })
      const saved = normalizeWindowsPath(updated.group_workspace_root ?? value)
      setRoot(saved)
      saveRememberedPrefix(PICKER_SCOPE, saved)
      go('provider')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t('workspace.saveError'))
    }
  }

  const chooseProvider = (provider: LLMProviderRead) => {
    setSelectedProvider(provider)
    setModel(provider.default_model)
    setCreatingProvider(false)
    go('model')
  }

  const saveAssistant = async () => {
    if (!selectedProvider || !model) return
    setError(null)
    try {
      await updateAssistant.mutateAsync({
        llm_provider_id: selectedProvider.id,
        model,
      })
      go('ready')
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t('model.saveError'))
    }
  }

  const finish = async () => {
    setError(null)
    try {
      await updateSettings.mutateAsync({
        onboarding_completed: true,
        language: normalizeLanguage(i18n.resolvedLanguage ?? i18n.language) ?? 'en-US',
      })
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t('ready.saveError'))
    }
  }

  return (
    <main className="relative flex h-full min-h-0 overflow-hidden bg-background">
      <aside className="relative hidden w-[36%] max-w-[32rem] min-w-[23rem] shrink-0 flex-col overflow-hidden bg-[#1a1816] p-9 text-[#fffaf2] lg:flex xl:p-11">
        <div aria-hidden className="pointer-events-none absolute inset-0 opacity-40 [background-image:linear-gradient(rgba(255,255,255,.035)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,.035)_1px,transparent_1px)] [background-size:44px_44px]" />
        <div className="absolute -left-20 top-[38%] h-72 w-72 rounded-full bg-[#d1502a]/15 blur-3xl" />
        <div className="absolute -right-16 -top-20 h-72 w-72 rounded-full bg-[#efb051]/10 blur-3xl" />
        <div className="relative flex items-center justify-between gap-4">
          <div className="flex items-center gap-3">
            <BrandMark animated className="h-11 w-11" />
            <span className="font-serif text-2xl font-semibold tracking-tight">Qunica</span>
          </div>
          <span className="rounded-full border border-white/10 bg-white/[0.045] px-3 py-1.5 text-[11px] font-medium text-[#fffaf2]/55">
            {t('time')}
          </span>
        </div>

        <div className="relative mt-auto mb-auto max-w-sm py-8">
          <p className="text-xs font-semibold uppercase tracking-[0.24em] text-[#efb051]">
            {t('eyebrow')}
          </p>
          <h1 className="mt-4 font-serif text-4xl font-semibold leading-tight tracking-[-0.025em]">
            {t('title')}
          </h1>
          <p className="mt-5 text-sm leading-7 text-[#fffaf2]/62">{t('description')}</p>
          <p className="mt-4 max-w-xs text-xs leading-5 text-[#fffaf2]/45">{t('localNote')}</p>

          <ol className="mt-6 space-y-2" aria-label={t('progress.label')}>
            {steps.map((step, index) => {
              const Icon = step.icon
              const complete = index < phaseIndex
              const active = index === phaseIndex
              return (
                <li
                  key={step.title}
                  aria-current={active ? 'step' : undefined}
                  className={cn(
                    'flex items-center gap-4 rounded-xl border px-4 py-3 transition-colors',
                    active
                      ? 'border-[#efb051]/35 bg-white/8'
                      : 'border-transparent text-[#fffaf2]/45',
                    complete && 'text-[#fffaf2]/75',
                  )}
                >
                  <span className={cn(
                    'flex h-9 w-9 shrink-0 items-center justify-center rounded-full border',
                    active && 'border-[#efb051]/50 bg-[#efb051]/12 text-[#efb051]',
                    complete && 'border-[#d1502a] bg-[#d1502a] text-white',
                    !active && !complete && 'border-white/15',
                  )}>
                    {complete ? <Check className="h-4 w-4" aria-hidden /> : <Icon className="h-4 w-4" aria-hidden />}
                  </span>
                  <span>
                    <span className="block text-sm font-medium">{step.title}</span>
                    <span className="mt-0.5 block text-xs text-[#fffaf2]/45">{step.detail}</span>
                  </span>
                </li>
              )
            })}
          </ol>
        </div>
      </aside>

      <section className="relative flex min-w-0 flex-1 flex-col overflow-hidden">
        <div className="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_92%_8%,color-mix(in_srgb,var(--color-primary)_9%,transparent),transparent_30%)]" />
        <header className="relative flex items-center justify-between border-b border-border/60 px-5 py-4 lg:hidden">
          <div className="flex items-center gap-2.5">
            <BrandMark className="h-8 w-8" />
            <span className="font-serif text-lg font-semibold">Qunica</span>
          </div>
          <span className="text-xs font-medium text-muted-foreground">
            {t('progress.compact', { current: Math.min(phaseIndex + 1, 3), total: 3 })}
          </span>
        </header>

        <div ref={contentRef} className="relative min-h-0 flex-1 overflow-y-auto px-5 py-10 sm:px-10 lg:px-14 lg:py-14">
          <div className="mx-auto w-full max-w-3xl animate-auth-rise">
            {phase === 'workspace' ? (
              <>
                <StepIntro
                  icon={FolderOpen}
                  eyebrow={t('workspace.eyebrow')}
                  title={t('workspace.title')}
                  description={t('workspace.description')}
                />
                <div className="mt-8 rounded-2xl border border-border/80 bg-card p-5 shadow-sm sm:p-7">
                  <label htmlFor="onboarding-workspace-root" className="text-sm font-semibold">
                    {t('workspace.directory')}
                  </label>
                  <p className="mt-1 text-xs leading-5 text-muted-foreground">{t('workspace.hint')}</p>
                  <div className="mt-4 flex flex-col gap-2 sm:flex-row">
                    <Input
                      ref={pathInputRef}
                      id="onboarding-workspace-root"
                      value={root}
                      placeholder={t('workspace.placeholder')}
                      aria-invalid={Boolean(error)}
                      onChange={(event) => {
                        setRoot(event.target.value)
                        setError(null)
                      }}
                      className="h-11 min-w-0 flex-1 font-mono text-xs"
                    />
                    <Button type="button" variant="outline" className="h-11" onClick={() => void onPickFolder()}>
                      <FolderOpen className="h-4 w-4" aria-hidden />
                      {t('workspace.pick')}
                    </Button>
                  </div>
                  {browsing ? (
                    <ServerFolderPicker
                      open
                      onOpenChange={setBrowsing}
                      onSelect={(absolutePath) => applyPick(absolutePath, absolutePath)}
                    />
                  ) : null}
                  <div className="mt-5 flex items-start gap-2 rounded-xl bg-muted/60 px-3.5 py-3 text-xs leading-5 text-muted-foreground">
                    <KeyRound className="mt-0.5 h-3.5 w-3.5 shrink-0 text-primary" aria-hidden />
                    <p>{t('workspace.privacy')}</p>
                  </div>
                </div>
                <StepFooter error={error}>
                  <Button className="h-10 px-5" disabled={updateSettings.isPending} onClick={() => void saveWorkspace()}>
                    {updateSettings.isPending ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : <ArrowRight className="h-4 w-4" aria-hidden />}
                    {updateSettings.isPending ? t('workspace.saving') : t('continue')}
                  </Button>
                </StepFooter>
              </>
            ) : null}

            {phase === 'provider' ? (
              <>
                <StepIntro
                  icon={Server}
                  eyebrow={t('provider.eyebrow')}
                  title={creatingProvider || (providers.data?.length ?? 0) === 0 ? t('provider.createTitle') : t('provider.title')}
                  description={creatingProvider || (providers.data?.length ?? 0) === 0 ? t('provider.createDescription') : t('provider.description')}
                />

                {providers.isLoading || assistant.isLoading ? (
                  <div className="mt-12 flex items-center gap-3 text-sm text-muted-foreground">
                    <Loader2 className="h-4 w-4 animate-spin text-primary" aria-hidden />
                    {t('provider.loading')}
                  </div>
                ) : providers.isError || assistant.isError ? (
                  <div className="mt-8 flex flex-wrap items-center justify-between gap-3 rounded-xl border border-destructive/30 bg-destructive/5 p-4">
                    <p role="alert" className="text-sm text-destructive">{t('provider.loadError')}</p>
                    <Button size="sm" variant="outline" onClick={() => void Promise.all([providers.refetch(), assistant.refetch()])}>
                      {t('retry')}
                    </Button>
                  </div>
                ) : creatingProvider || (providers.data?.length ?? 0) === 0 ? (
                  <div className="mt-8 rounded-2xl border border-border/80 bg-card p-5 shadow-sm sm:p-7">
                    <Suspense fallback={
                      <div className="flex items-center gap-3 py-8 text-sm text-muted-foreground">
                        <Loader2 className="h-4 w-4 animate-spin text-primary" aria-hidden />
                        {t('provider.loadingForm')}
                      </div>
                    }>
                      <CreateProviderForm onCreated={chooseProvider} />
                    </Suspense>
                  </div>
                ) : (
                  <div className="mt-8 grid gap-3 sm:grid-cols-2">
                    {providers.data?.map((provider) => (
                      <button
                        key={provider.id}
                        type="button"
                        onClick={() => chooseProvider(provider)}
                        className="group flex min-h-28 items-start gap-4 rounded-2xl border border-border/80 bg-card p-5 text-left shadow-xs transition-[border-color,transform,box-shadow] hover:-translate-y-0.5 hover:border-primary/45 hover:shadow-md focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                      >
                        <span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
                          <Server className="h-4 w-4" aria-hidden />
                        </span>
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm font-semibold">{provider.name}</span>
                          <span className="mt-1 block truncate font-mono text-xs text-muted-foreground">{provider.default_model}</span>
                        </span>
                        <ArrowRight className="mt-3 h-4 w-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5 group-hover:text-primary" aria-hidden />
                      </button>
                    ))}
                    <button
                      type="button"
                      onClick={() => setCreatingProvider(true)}
                      className="flex min-h-28 items-center justify-center gap-2 rounded-2xl border border-dashed border-border bg-transparent p-5 text-sm font-medium text-muted-foreground transition-colors hover:border-primary/45 hover:bg-primary/5 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    >
                      <Plus className="h-4 w-4" aria-hidden />
                      {t('provider.createAnother')}
                    </button>
                  </div>
                )}

                <StepFooter error={error}>
                  <Button variant="ghost" onClick={() => creatingProvider && (providers.data?.length ?? 0) > 0 ? setCreatingProvider(false) : go('workspace')}>
                    <ArrowLeft className="h-4 w-4" aria-hidden />
                    {creatingProvider && (providers.data?.length ?? 0) > 0 ? t('provider.backToProviders') : t('back')}
                  </Button>
                </StepFooter>
              </>
            ) : null}

            {phase === 'model' && selectedProvider ? (
              <>
                <StepIntro
                  icon={Bot}
                  eyebrow={t('model.eyebrow')}
                  title={t('model.title')}
                  description={t('model.description', { provider: selectedProvider.name })}
                />
                <fieldset className="mt-8 space-y-3">
                  <legend className="sr-only">{t('model.legend')}</legend>
                  {modelOptions.map((option) => {
                    const selected = model === option
                    return (
                      <label
                        key={option}
                        className={cn(
                          'flex cursor-pointer items-center gap-4 rounded-2xl border bg-card p-4 shadow-xs transition-[border-color,background-color,transform]',
                          selected ? 'border-primary bg-primary/5' : 'border-border/80 hover:-translate-y-px hover:border-primary/35',
                        )}
                      >
                        <input
                          type="radio"
                          name="assistant-model"
                          value={option}
                          checked={selected}
                          onChange={() => setModel(option)}
                          className="sr-only"
                        />
                        <span className={cn(
                          'flex h-9 w-9 shrink-0 items-center justify-center rounded-full border',
                          selected ? 'border-primary bg-primary text-primary-foreground' : 'border-border text-transparent',
                        )}>
                          <Check className="h-4 w-4" aria-hidden />
                        </span>
                        <span className="min-w-0 flex-1 truncate font-mono text-sm">{option}</span>
                        {option === selectedProvider.default_model ? (
                          <span className="rounded-full bg-muted px-2.5 py-1 text-2xs font-medium text-muted-foreground">{t('model.default')}</span>
                        ) : null}
                      </label>
                    )
                  })}
                </fieldset>
                <StepFooter error={error}>
                  <Button variant="ghost" onClick={() => go('provider')}>
                    <ArrowLeft className="h-4 w-4" aria-hidden />
                    {t('back')}
                  </Button>
                  <Button className="h-10 px-5" disabled={!model || updateAssistant.isPending} onClick={() => void saveAssistant()}>
                    {updateAssistant.isPending ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : <ArrowRight className="h-4 w-4" aria-hidden />}
                    {updateAssistant.isPending ? t('model.saving') : t('continue')}
                  </Button>
                </StepFooter>
              </>
            ) : null}

            {phase === 'ready' && selectedProvider ? (
              <>
                <StepIntro
                  icon={Sparkles}
                  eyebrow={t('ready.eyebrow')}
                  title={t('ready.title')}
                  description={t('ready.description')}
                />
                <div className="mt-8 overflow-hidden rounded-2xl border border-border/80 bg-card shadow-sm">
                  {[
                    [t('ready.workspace'), root],
                    [t('ready.provider'), selectedProvider.name],
                    [t('ready.model'), model],
                  ].map(([label, value]) => (
                    <div key={label} className="flex items-center gap-4 border-b border-border/70 px-5 py-4 last:border-b-0">
                      <CheckCircle2 className="h-5 w-5 shrink-0 text-primary" aria-hidden />
                      <span className="w-24 shrink-0 text-xs font-medium text-muted-foreground">{label}</span>
                      <span className="min-w-0 flex-1 truncate font-mono text-xs">{value}</span>
                    </div>
                  ))}
                </div>
                <div className="mt-4 flex items-start gap-3 rounded-2xl border border-primary/20 bg-primary/5 px-4 py-4 sm:px-5">
                  <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-primary/10 text-primary">
                    <MessagesSquare className="h-4 w-4" aria-hidden />
                  </span>
                  <div>
                    <p className="text-sm font-semibold">{t('ready.nextTitle')}</p>
                    <p className="mt-1 text-xs leading-5 text-muted-foreground">{t('ready.nextDescription')}</p>
                  </div>
                </div>
                <StepFooter error={error}>
                  <Button variant="ghost" onClick={() => go('model')}>
                    <ArrowLeft className="h-4 w-4" aria-hidden />
                    {t('back')}
                  </Button>
                  <Button className="h-10 px-5" disabled={updateSettings.isPending} onClick={() => void finish()}>
                    {updateSettings.isPending ? <Loader2 className="h-4 w-4 animate-spin" aria-hidden /> : <Sparkles className="h-4 w-4" aria-hidden />}
                    {updateSettings.isPending ? t('ready.entering') : t('ready.enter')}
                  </Button>
                </StepFooter>
              </>
            ) : null}
          </div>
        </div>
      </section>
    </main>
  )
}

function StepIntro({ icon: Icon, eyebrow, title, description }: {
  icon: typeof FolderOpen
  eyebrow: string
  title: string
  description: string
}) {
  return (
    <div className="max-w-2xl">
      <span className="flex h-12 w-12 items-center justify-center rounded-2xl bg-primary/10 text-primary shadow-xs">
        <Icon className="h-5 w-5" aria-hidden />
      </span>
      <p className="mt-6 text-xs font-semibold uppercase tracking-[0.2em] text-primary">{eyebrow}</p>
      <h2 data-onboarding-title tabIndex={-1} className="mt-2 font-serif text-3xl font-semibold tracking-[-0.02em] outline-none sm:text-4xl">{title}</h2>
      <p className="mt-4 max-w-xl text-sm leading-7 text-muted-foreground">{description}</p>
    </div>
  )
}

function StepFooter({ error, children }: { error: string | null; children: ReactNode }) {
  return (
    <div className="mt-8 flex min-h-10 flex-wrap items-center justify-between gap-3 border-t border-border/60 pt-6">
      <p className="min-w-0 flex-1 text-sm text-destructive" role={error ? 'alert' : undefined}>{error}</p>
      <div className="flex items-center gap-2">{children}</div>
    </div>
  )
}
