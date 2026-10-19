import { useEffect, useState, type FormEvent } from 'react'
import { CalendarClock, Loader2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Switch } from '@/components/ui/switch'
import { Textarea } from '@/components/ui/textarea'
import {
  useCreateAutomation,
  usePreviewAutomation,
  useUpdateAutomation,
} from '@/hooks/useAutomations'
import {
  browserTimezone,
  defaultOnceAt,
  formatAutomationTime,
  fromLocalDateTimeInput,
  toLocalDateTimeInput,
} from '@/lib/automationTime'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'
import type {
  AutomationConfigInput,
  AutomationMissedPolicy,
  AutomationRead,
  AutomationTrigger,
  GroupRead,
} from '@/types/api'

type TriggerKind = AutomationTrigger['kind']
type DraftErrorKey =
  | 'group'
  | 'name'
  | 'prompt'
  | 'cron'
  | 'timezone'
  | 'once'
  | 'githubEvents'
  | 'steps'
  | 'tokens'
  | 'timeout'

interface Draft {
  groupId: string
  name: string
  prompt: string
  kind: TriggerKind
  cron: string
  onceAt: string
  githubEvents: string
  timezone: string
  missedPolicy: AutomationMissedPolicy
  steps: string
  tokens: string
  timeout: string
  enabled: boolean
}

const TIMEZONES = [
  'UTC',
  'Asia/Shanghai',
  'Asia/Tokyo',
  'Europe/London',
  'Europe/Berlin',
  'America/New_York',
  'America/Los_Angeles',
]

function emptyDraft(groupId: string): Draft {
  return {
    groupId,
    name: '',
    prompt: '',
    kind: 'cron',
    cron: '0 9 * * *',
    onceAt: defaultOnceAt(),
    githubEvents: '',
    timezone: browserTimezone(),
    missedPolicy: 'coalesce_latest',
    steps: '8',
    tokens: '64000',
    timeout: '300',
    enabled: true,
  }
}

function draftFrom(automation: AutomationRead): Draft {
  const trigger = automation.trigger
  return {
    groupId: automation.group_id,
    name: automation.name,
    prompt: automation.prompt,
    kind: trigger.kind,
    cron: trigger.kind === 'cron' ? trigger.expression : '0 9 * * *',
    onceAt: trigger.kind === 'once' ? toLocalDateTimeInput(trigger.at) : defaultOnceAt(),
    githubEvents: trigger.kind === 'github' ? trigger.events.join(', ') : '',
    timezone: automation.timezone,
    missedPolicy: automation.missed_policy,
    steps: String(automation.budget.max_agent_steps),
    tokens: String(automation.budget.max_total_tokens),
    timeout: String(automation.budget.turn_timeout_seconds),
    enabled: automation.enabled,
  }
}

function triggerFromDraft(draft: Draft): AutomationTrigger | null {
  if (draft.kind === 'webhook') return { kind: 'webhook' }
  if (draft.kind === 'github') {
    return {
      kind: 'github',
      events: draft.githubEvents
        .split(',')
        .map((event) => event.trim())
        .filter(Boolean),
    }
  }
  if (draft.kind === 'once') {
    const at = fromLocalDateTimeInput(draft.onceAt)
    return at ? { kind: 'once', at } : null
  }
  const expression = draft.cron.trim()
  return expression ? { kind: 'cron', expression } : null
}

export function AutomationFormDialog({
  open,
  onOpenChange,
  automation,
  groups,
  onSaved,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  automation: AutomationRead | null
  groups: GroupRead[]
  onSaved: (automation: AutomationRead) => void
}) {
  const { t, i18n } = useTranslation('automations')
  const locale = i18n.resolvedLanguage ?? 'en-US'
  const create = useCreateAutomation()
  const update = useUpdateAutomation()
  const { mutateAsync: previewSchedule } = usePreviewAutomation()
  const [draft, setDraft] = useState<Draft>(() =>
    automation ? draftFrom(automation) : emptyDraft(groups[0]?.id ?? ''),
  )
  const [errors, setErrors] = useState<Partial<Record<DraftErrorKey, string>>>({})
  const [preview, setPreview] = useState<string[]>([])
  const [previewError, setPreviewError] = useState<string | null>(null)
  const [previewLoading, setPreviewLoading] = useState(false)
  const [saveError, setSaveError] = useState<string | null>(null)
  const saving = create.isPending || update.isPending

  const change = (patch: Partial<Draft>) => {
    setDraft((current) => ({ ...current, ...patch }))
    setErrors((current) => {
      const next = { ...current }
      for (const key of Object.keys(patch) as (keyof Draft)[]) {
        delete next[key as DraftErrorKey]
      }
      return next
    })
  }

  const kind = draft.kind
  const cron = draft.cron
  const onceAt = draft.onceAt
  const timezone = draft.timezone.trim()
  useEffect(() => {
    let trigger: AutomationTrigger | null = null
    if (kind === 'cron' && cron.trim().split(/\s+/).length === 5) {
      trigger = { kind: 'cron', expression: cron.trim() }
    } else if (kind === 'once') {
      const at = fromLocalDateTimeInput(onceAt)
      if (at) trigger = { kind: 'once', at }
    }
    if (!open || !trigger) {
      setPreview([])
      setPreviewError(null)
      setPreviewLoading(false)
      return
    }
    let cancelled = false
    const timer = window.setTimeout(() => {
      setPreviewLoading(true)
      void previewSchedule({ trigger, timezone })
        .then((result) => {
          if (cancelled) return
          setPreview(result.preview)
          setPreviewError(null)
        })
        .catch((error: unknown) => {
          if (cancelled) return
          setPreview([])
          setPreviewError(
            error instanceof ApiError ? error.message : t('errors.network'),
          )
        })
        .finally(() => {
          if (!cancelled) setPreviewLoading(false)
        })
    }, 450)
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [cron, kind, onceAt, open, previewSchedule, t, timezone])

  const validate = (): boolean => {
    const next: Partial<Record<DraftErrorKey, string>> = {}
    if (!draft.groupId) next.group = t('validation.group')
    if (!draft.name.trim()) next.name = t('validation.name')
    if (!draft.prompt.trim()) next.prompt = t('validation.prompt')
    if (draft.kind === 'cron' && draft.cron.trim().split(/\s+/).length !== 5) {
      next.cron = t('validation.cron')
    }
    if (draft.kind === 'once') {
      const at = fromLocalDateTimeInput(draft.onceAt)
      if (!at || new Date(at).getTime() <= Date.now()) next.once = t('validation.once')
    }
    if (draft.kind !== 'webhook' && draft.kind !== 'github') {
      const timezone = draft.timezone.trim()
      let valid = false
      try {
        new Intl.DateTimeFormat('en-US', { timeZone: timezone })
        valid = Boolean(timezone)
      } catch {
        valid = false
      }
      if (!valid) next.timezone = t('validation.timezone')
    }
    if (draft.kind === 'github') {
      const events = draft.githubEvents
        .split(',')
        .map((event) => event.trim())
        .filter(Boolean)
      if (
        events.length > 32 ||
        events.some(
          (event) =>
            event.length > 64 ||
            !/^[A-Za-z0-9_.-]+$/.test(event),
        )
      ) {
        next.githubEvents = t('validation.githubEvents')
      }
    }
    const steps = Number(draft.steps)
    if (!Number.isInteger(steps) || steps < 1 || steps > 100) next.steps = t('validation.steps')
    const tokens = Number(draft.tokens)
    if (!Number.isInteger(tokens) || tokens < 1 || tokens > 1_000_000) {
      next.tokens = t('validation.tokens')
    }
    const timeout = Number(draft.timeout)
    if (!Number.isInteger(timeout) || timeout < 1 || timeout > 3600) {
      next.timeout = t('validation.timeout')
    }
    setErrors(next)
    return Object.keys(next).length === 0
  }

  const submit = async (event: FormEvent) => {
    event.preventDefault()
    setSaveError(null)
    if (!validate()) return
    const trigger = triggerFromDraft(draft)
    if (!trigger) return
    const config: AutomationConfigInput = {
      group_id: draft.groupId,
      name: draft.name.trim(),
      prompt: draft.prompt.trim(),
      trigger,
      timezone: draft.timezone.trim() || browserTimezone(),
      missed_policy: draft.missedPolicy,
      budget: {
        max_agent_steps: Number(draft.steps),
        max_total_tokens: Number(draft.tokens),
        turn_timeout_seconds: Number(draft.timeout),
      },
      enabled: draft.enabled,
      expected_version: automation?.version ?? null,
    }
    try {
      const saved = automation
        ? await update.mutateAsync({ id: automation.id, config })
        : await create.mutateAsync(config)
      onSaved(saved)
      onOpenChange(false)
    } catch (error) {
      setSaveError(
        error instanceof ApiError && error.code === 'conflict'
          ? t('errors.conflict')
          : error instanceof ApiError
            ? t('errors.save', { message: error.message })
            : t('errors.network'),
      )
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[90vh] max-w-2xl gap-4">
        <DialogHeader>
          <DialogTitle>
            {automation ? t('editTitle') : t('createTitle')}
          </DialogTitle>
          <DialogDescription>{t('form.description')}</DialogDescription>
        </DialogHeader>
        <form className="flex min-h-0 flex-col gap-4" onSubmit={submit} noValidate>
          <div className="grid max-h-[64vh] gap-4 overflow-y-auto pr-1 sm:grid-cols-2">
            <FormField
              name="automation-group"
              label={t('fields.group')}
              required
              error={errors.group}
            >
              {(props) => (
                <Select
                  value={draft.groupId}
                  onValueChange={(groupId) => change({ groupId })}
                >
                  <SelectTrigger {...props}>
                    <SelectValue placeholder={t('fields.group')} />
                  </SelectTrigger>
                  <SelectContent>
                    {groups.map((group) => (
                      <SelectItem key={group.id} value={group.id}>
                        {group.name}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </FormField>
            <FormField
              name="automation-name"
              label={t('fields.name')}
              required
              error={errors.name}
            >
              {(props) => (
                <Input
                  {...props}
                  value={draft.name}
                  maxLength={120}
                  placeholder={t('fields.namePlaceholder')}
                  onChange={(event) => change({ name: event.target.value })}
                />
              )}
            </FormField>

            <FormField
              name="automation-prompt"
              label={t('fields.prompt')}
              required
              error={errors.prompt}
              className="sm:col-span-2"
            >
              {(props) => (
                <Textarea
                  {...props}
                  rows={4}
                  value={draft.prompt}
                  maxLength={16 * 1024}
                  placeholder={t('fields.promptPlaceholder')}
                  onChange={(event) => change({ prompt: event.target.value })}
                />
              )}
            </FormField>

            <FormField name="automation-kind" label={t('fields.kind')}>
              {(props) => (
                <Select
                  value={draft.kind}
                  onValueChange={(kind) => change({ kind: kind as TriggerKind })}
                >
                  <SelectTrigger {...props}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="cron">{t('fields.cron')}</SelectItem>
                    <SelectItem value="once">{t('fields.once')}</SelectItem>
                    <SelectItem value="webhook">{t('fields.webhook')}</SelectItem>
                    <SelectItem value="github">{t('fields.github')}</SelectItem>
                  </SelectContent>
                </Select>
              )}
            </FormField>

            {draft.kind === 'cron' ? (
              <FormField
                name="automation-cron"
                label={t('fields.cronExpression')}
                required
                description={t('fields.cronHint')}
                error={errors.cron}
              >
                {(props) => (
                  <Input
                    {...props}
                    value={draft.cron}
                    maxLength={128}
                    className="font-mono"
                    placeholder="0 9 * * MON-FRI"
                    onChange={(event) => change({ cron: event.target.value })}
                  />
                )}
              </FormField>
            ) : null}

            {draft.kind === 'once' ? (
              <FormField
                name="automation-once"
                label={t('fields.onceAt')}
                required
                error={errors.once}
              >
                {(props) => (
                  <Input
                    {...props}
                    type="datetime-local"
                    value={draft.onceAt}
                    onChange={(event) => change({ onceAt: event.target.value })}
                  />
                )}
              </FormField>
            ) : null}

            {draft.kind === 'github' ? (
              <FormField
                name="automation-github-events"
                label={t('fields.githubEvents')}
                description={t('fields.githubEventsHint')}
                error={errors.githubEvents}
              >
                {(props) => (
                  <Input
                    {...props}
                    value={draft.githubEvents}
                    className="font-mono text-xs"
                    placeholder="push, pull_request"
                    onChange={(event) => change({ githubEvents: event.target.value })}
                  />
                )}
              </FormField>
            ) : null}

            {draft.kind !== 'webhook' && draft.kind !== 'github' ? (
              <FormField
                name="automation-timezone"
                label={t('fields.timezone')}
                required
                description={t('fields.timezoneHint')}
                error={errors.timezone}
              >
                {(props) => (
                  <>
                    <Input
                      {...props}
                      value={draft.timezone}
                      list="automation-timezones"
                      className="font-mono text-xs"
                      onChange={(event) => change({ timezone: event.target.value })}
                    />
                    <datalist id="automation-timezones">
                      {TIMEZONES.map((timezone) => (
                        <option key={timezone} value={timezone} />
                      ))}
                    </datalist>
                  </>
                )}
              </FormField>
            ) : (
              <p className="self-end text-xs leading-relaxed text-muted-foreground sm:col-span-1">
                {draft.kind === 'github' ? t('fields.githubHint') : t('fields.webhookHint')}
              </p>
            )}

            <FormField
              name="automation-missed"
              label={t('fields.missedPolicy')}
              description={t('fields.missedPolicyHint')}
            >
              {(props) => (
                <Select
                  value={draft.missedPolicy}
                  onValueChange={(missedPolicy) =>
                    change({ missedPolicy: missedPolicy as AutomationMissedPolicy })
                  }
                >
                  <SelectTrigger {...props}>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="coalesce_latest">{t('fields.coalesce')}</SelectItem>
                    <SelectItem value="skip">{t('fields.skip')}</SelectItem>
                  </SelectContent>
                </Select>
              )}
            </FormField>

            <div className="space-y-2 sm:col-span-2">
              <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                {t('fields.budget')}
              </p>
              <div className="grid gap-3 sm:grid-cols-3">
                <FormField
                  name="automation-steps"
                  label={t('fields.steps')}
                  error={errors.steps}
                >
                  {(props) => (
                    <Input
                      {...props}
                      type="number"
                      min={1}
                      max={100}
                      value={draft.steps}
                      onChange={(event) => change({ steps: event.target.value })}
                    />
                  )}
                </FormField>
                <FormField
                  name="automation-tokens"
                  label={t('fields.tokens')}
                  error={errors.tokens}
                >
                  {(props) => (
                    <Input
                      {...props}
                      type="number"
                      min={1}
                      max={1_000_000}
                      value={draft.tokens}
                      onChange={(event) => change({ tokens: event.target.value })}
                    />
                  )}
                </FormField>
                <FormField
                  name="automation-timeout"
                  label={`${t('fields.timeout')} (${t('fields.seconds')})`}
                  error={errors.timeout}
                >
                  {(props) => (
                    <Input
                      {...props}
                      type="number"
                      min={1}
                      max={3600}
                      value={draft.timeout}
                      onChange={(event) => change({ timeout: event.target.value })}
                    />
                  )}
                </FormField>
              </div>
            </div>

            <div className="flex items-center justify-between gap-4 rounded-lg border border-border/70 bg-muted/30 p-3 sm:col-span-2">
              <div className="min-w-0">
                <label htmlFor="automation-enabled" className="text-sm font-medium">
                  {t('fields.enabled')}
                </label>
                <p className="mt-0.5 text-xs text-muted-foreground">
                  {t('fields.enabledHint')}
                </p>
              </div>
              <Switch
                id="automation-enabled"
                checked={draft.enabled}
                onCheckedChange={(enabled) => change({ enabled })}
              />
            </div>

            <div className="rounded-lg border border-border/70 bg-muted/20 p-3 sm:col-span-2">
              <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                {t('fields.permissions')}
              </p>
              {automation ? (
                automation.permissions.length > 0 ? (
                  <ul className="mt-1.5 flex flex-wrap gap-1.5">
                    {automation.permissions.map((permission) => (
                      <li key={permission.agent_id}>
                        <Badge
                          variant={permission.bypass_approvals ? 'warning' : 'outline'}
                          className="font-normal"
                          title={
                            permission.bypass_approvals
                              ? t('fields.bypassApprovals')
                              : undefined
                          }
                        >
                          {permission.name}
                          {permission.bypass_approvals
                            ? ` · ${t('fields.bypassApprovals')}`
                            : ''}
                        </Badge>
                      </li>
                    ))}
                  </ul>
                ) : (
                  <p className="mt-1.5 text-xs text-muted-foreground">
                    {t('fields.noPermissions')}
                  </p>
                )
              ) : (
                <p className="mt-1.5 text-xs text-muted-foreground">
                  {t('fields.permissionsHint')}
                </p>
              )}
            </div>

            <div
              aria-live="polite"
              className="rounded-lg border border-border/70 bg-muted/20 p-3 sm:col-span-2"
            >
              <div className="flex items-center gap-2">
                <CalendarClock className="h-4 w-4 text-primary" aria-hidden />
                <p className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
                  {t('preview.title')}
                </p>
                {previewLoading ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" aria-hidden />
                ) : null}
              </div>
              {draft.kind === 'webhook' || draft.kind === 'github' ? (
                <p className="mt-2 text-xs text-muted-foreground">
                  {draft.kind === 'github' ? t('preview.github') : t('preview.webhook')}
                </p>
              ) : previewLoading && preview.length === 0 ? (
                <p className="mt-2 text-xs text-muted-foreground">{t('preview.loading')}</p>
              ) : previewError ? (
                <p role="alert" className="mt-2 text-xs text-destructive">
                  {t('errors.preview', { message: previewError })}
                </p>
              ) : preview.length === 0 ? (
                <p className="mt-2 text-xs text-muted-foreground">{t('preview.empty')}</p>
              ) : (
                <ol className="mt-2 grid gap-1 font-mono text-xs tabular-nums text-foreground/80 sm:grid-cols-2">
                  {preview.map((run) => (
                    <li key={run} className="truncate">
                      {formatAutomationTime(run, locale)}
                    </li>
                  ))}
                </ol>
              )}
            </div>

            {saveError ? (
              <p
                role="alert"
                className={cn(
                  'rounded-md border border-destructive/30 bg-destructive/10 px-3 py-2 text-xs text-destructive sm:col-span-2',
                )}
              >
                {saveError}
              </p>
            ) : null}
          </div>

          <DialogFooter>
            <Button
              type="button"
              variant="ghost"
              onClick={() => onOpenChange(false)}
              disabled={saving}
            >
              {t('form.cancel')}
            </Button>
            <Button type="submit" disabled={saving}>
              {saving
                ? t('form.saving')
                : automation
                  ? t('form.save')
                  : t('form.create')}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
