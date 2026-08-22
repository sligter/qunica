import { useEffect, useState } from 'react'
import {
  Check,
  Copy,
  Loader2,
  Server,
  Wrench,
} from 'lucide-react'
import { useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import {
  EDIT_MCP_SERVER_FORM_ID,
  McpServerForm,
} from '@/components/mcp/McpServerForm'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { ConfirmDialog } from '@/components/ui/confirm-dialog'
import { Field, FieldGrid } from '@/components/ui/field'
import { PageState } from '@/components/ui/page-state'
import { DetailSkeleton } from '@/components/ui/skeleton'
import { Section } from '@/components/ui/section'
import {
  useDeleteMcpServer,
  useMcpServer,
  useTestMcpServer,
} from '@/hooks/useMcpServers'
import { useEditSaveGuard } from '@/hooks/useEditSaveGuard'
import type { McpTransport } from '@/types/api'
import { cn } from '@/lib/utils'

const TRANSPORT_KEYS: Record<McpTransport, 'stdio' | 'streamableHttp' | 'sse'> = {
  stdio: 'stdio',
  'streamable-http': 'streamableHttp',
  sse: 'sse',
}

function transportBadgeClass(transport: McpTransport): string {
  if (transport === 'stdio') {
    return 'bg-purple-500/10 text-purple-600 dark:text-purple-400 border-purple-500/20'
  }
  if (transport === 'sse') {
    return 'bg-blue-500/10 text-blue-600 dark:text-blue-400 border-blue-500/20'
  }
  return 'bg-amber-500/10 text-amber-600 dark:text-amber-400 border-amber-500/20'
}

export function McpServerDetailPage() {
  const { t } = useTranslation(['mcp', 'common'])
  const { serverId } = useParams<{ serverId: string }>()
  const server = useMcpServer(serverId)
  const del = useDeleteMcpServer()
  const test = useTestMcpServer()
  const navigate = useNavigate()
  const [searchParams, setSearchParams] = useSearchParams()
  // Deep link: /mcp-servers/:id?edit=1 opens straight into the edit form.
  const [editing, setEditing] = useState(searchParams.get('edit') === '1')
  const [saving, setSaving] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const [copiedConfig, setCopiedConfig] = useState(false)
  const saveReady = useEditSaveGuard(editing)

  useEffect(() => {
    if (editing) {
      setSearchParams(new URLSearchParams({ edit: '1' }), { replace: true })
    } else {
      setSearchParams({}, { replace: true })
    }
  }, [editing, setSearchParams])

  if (server.isLoading) {
    return <DetailSkeleton label={t('mcp:detail.loading')} />
  }
  if (server.error) {
    return (
      <PageState
        variant="error"
        title={t('mcp:detail.loadError', { error: String(server.error) })}
      />
    )
  }
  if (!server.data) {
    return <PageState title={t('mcp:detail.notFound')} />
  }

  const s = server.data

  if (editing) {
    return (
      <DetailShell
        title={t('mcp:detail.editTitle', { name: s.name })}
        actions={
          <>
            <Button
              size="sm"
              type="submit"
              form={EDIT_MCP_SERVER_FORM_ID}
              disabled={!saveReady || saving}
            >
              {saving
                ? t('common:actions.saving')
                : t('mcp:actions.saveChanges')}
            </Button>
            <Button size="sm" variant="ghost" onClick={() => setEditing(false)}>
              {t('common:actions.cancel')}
            </Button>
          </>
        }
      >
        <McpServerForm
          server={s}
          onSavingChange={setSaving}
          onSaved={() => setEditing(false)}
        />
      </DetailShell>
    )
  }

  const endpoint = s.transport === 'stdio' ? (s.command ?? '-') : (s.url ?? '-')
  const result = test.data

  const onCopyConfigJson = () => {
    const configSnippet =
      s.transport === 'stdio'
        ? {
            command: s.command,
            args: s.args,
            env: s.env,
          }
        : {
            url: s.url,
          }
    if (!navigator.clipboard) return
    void navigator.clipboard
      .writeText(JSON.stringify({ [s.slug || s.name]: configSnippet }, null, 2))
      .then(() => {
        setCopiedConfig(true)
        setTimeout(() => setCopiedConfig(false), 2000)
      })
  }

  return (
    <DetailShell
      title={s.name}
      subtitle={
        <div className="flex flex-wrap items-center gap-2">
          <span>{`${t(`mcp:transports.${TRANSPORT_KEYS[s.transport]}.label`)} · ${endpoint}`}</span>
          <Badge variant={s.enabled ? 'default' : 'secondary'} className="text-[10px]">
            {s.enabled ? t('mcp:states.enabled') : t('mcp:states.disabled')}
          </Badge>
        </div>
      }
      actions={
        <>
          <Button
            variant="outline"
            size="sm"
            disabled={test.isPending}
            onClick={() => test.mutate(s.id)}
            className="gap-1.5"
          >
            {test.isPending ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Wrench className="h-3.5 w-3.5" />}
            {t('mcp:actions.test')}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              setSaving(false)
              setEditing(true)
            }}
          >
            {t('common:actions.edit')}
          </Button>
          <Button
            variant="destructive"
            size="sm"
            onClick={() => setConfirmOpen(true)}
            disabled={del.isPending}
          >
            {del.isPending ? t('common:actions.deleting') : t('common:actions.delete')}
          </Button>
        </>
      }
    >
      <div className="space-y-6">
        <div className="flex flex-col sm:flex-row items-start sm:items-center justify-between gap-4 rounded-xl border border-border/80 bg-card/60 p-4 shadow-xs">
          <div className="flex items-center gap-3.5">
            <span className="flex h-12 w-12 shrink-0 select-none items-center justify-center rounded-2xl bg-primary/10 text-primary shadow-xs">
              <Server className="h-6 w-6" />
            </span>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-base font-semibold">{s.name}</h2>
                <span
                  className={cn(
                    'inline-block rounded-md border px-1.5 py-0.5 text-[10px] font-medium leading-none uppercase',
                    transportBadgeClass(s.transport),
                  )}
                >
                  {s.transport}
                </span>
              </div>
              <code className="text-xs font-mono text-muted-foreground mt-0.5 block truncate max-w-md">
                {endpoint}
              </code>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            onClick={onCopyConfigJson}
            className="h-8 gap-1.5 text-xs text-muted-foreground"
          >
            {copiedConfig ? <Check className="h-3.5 w-3.5 text-success" /> : <Copy className="h-3.5 w-3.5" />}
            <span>{copiedConfig ? t('common:actions.copied', '已复制 JSON') : t('mcp:actions.copyConfig', '复制配置 JSON')}</span>
          </Button>
        </div>

        <FieldGrid columns={3}>
          <Field
            label={t('mcp:fields.transport')}
            value={t(`mcp:transports.${TRANSPORT_KEYS[s.transport]}.label`)}
          />
          <Field label={t('mcp:fields.toolPrefix')} value={`mcp__${s.slug}__`} mono />
          <Field label={t('mcp:fields.timeout')} value={`${s.timeout_seconds}s`} />
          {s.transport === 'stdio' ? (
            <>
              <Field label={t('mcp:fields.command')} value={s.command ?? '-'} mono />
              <Field
                label={t('mcp:fields.args')}
                value={s.args.length > 0 ? s.args.join(' ') : '-'}
                mono
              />
              <Field label={t('mcp:fields.cwd')} value={s.cwd ?? '-'} mono />
            </>
          ) : (
            <>
              <Field label={t('mcp:fields.url')} value={s.url ?? '-'} mono />
              <Field
                label={t('mcp:fields.headers')}
                value={
                  Object.keys(s.headers_masked).length > 0
                    ? Object.entries(s.headers_masked)
                        .map(([key, value]) => `${key}: ${value}`)
                        .join(', ')
                    : '-'
                }
                mono
              />
            </>
          )}
          <Field
            label={t('mcp:fields.toolFilter')}
            value={s.tool_filter.length > 0 ? s.tool_filter.join(', ') : t('mcp:states.allTools')}
          />
        </FieldGrid>

        {s.transport === 'stdio' && Object.keys(s.env).length > 0 ? (
          <Section title={t('mcp:fields.env')} as="h3">
            <div className="rounded-xl border border-border/80 bg-code p-3 font-mono text-xs text-code-foreground">
              <ul className="space-y-1">
                {Object.entries(s.env).map(([key, value]) => (
                  <li key={key} className="truncate">
                    <span className="text-primary font-medium">{key}</span>={value}
                  </li>
                ))}
              </ul>
            </div>
          </Section>
        ) : null}

        {result ? (
          <Section
            title={t('mcp:test.title')}
            description={
              result.ok
                ? result.server_label
                  ? t('mcp:test.connectedTo', { server: result.server_label })
                  : t('mcp:test.connected')
                : t('mcp:test.failed')
            }
            as="h3"
          >
            {result.ok ? (
              result.tools.length === 0 ? (
                <p className="text-sm text-muted-foreground">{t('mcp:test.noTools')}</p>
              ) : (
                <div className="grid gap-2.5 sm:grid-cols-2">
                  {result.tools.map((tool) => (
                    <div
                      key={tool.exposed_name}
                      className="rounded-xl border border-border/80 bg-card p-3.5 shadow-xs transition-colors hover:border-primary/40"
                    >
                      <div className="flex items-center gap-2">
                        <Wrench className="h-3.5 w-3.5 text-primary shrink-0" />
                        <code className="font-mono text-xs font-semibold text-foreground truncate">
                          {tool.exposed_name}
                        </code>
                      </div>
                      {tool.description ? (
                        <p className="mt-1.5 text-xs text-muted-foreground line-clamp-2">
                          {tool.description}
                        </p>
                      ) : null}
                    </div>
                  ))}
                </div>
              )
            ) : (
              <div className="rounded-xl border border-destructive/30 bg-destructive/10 p-4 text-sm text-destructive">
                <p className="break-words font-medium">{result.error}</p>
              </div>
            )}
          </Section>
        ) : null}

        {s.description ? (
          <Section title={t('mcp:detail.description')} as="h3">
            <p className="whitespace-pre-wrap text-sm leading-relaxed text-muted-foreground bg-card p-4 rounded-xl border border-border/80">
              {s.description}
            </p>
          </Section>
        ) : null}
      </div>

      <ConfirmDialog
        open={confirmOpen}
        onOpenChange={setConfirmOpen}
        title={t('mcp:detail.deleteTitle', { name: s.name })}
        description={t('mcp:detail.deleteDescription')}
        confirmLabel={t('common:actions.delete')}
        destructive
        onConfirm={async () => {
          await del.mutateAsync(s.id)
          void navigate('/mcp-servers')
        }}
      />
    </DetailShell>
  )
}
