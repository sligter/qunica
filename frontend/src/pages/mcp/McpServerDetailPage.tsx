import { useState } from 'react'
import { Loader2 } from 'lucide-react'
import { useNavigate, useParams } from 'react-router-dom'
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

const TRANSPORT_KEYS: Record<McpTransport, 'stdio' | 'streamableHttp' | 'sse'> = {
  stdio: 'stdio',
  'streamable-http': 'streamableHttp',
  sse: 'sse',
}

export function McpServerDetailPage() {
  const { t } = useTranslation(['mcp', 'common'])
  const { serverId } = useParams<{ serverId: string }>()
  const server = useMcpServer(serverId)
  const del = useDeleteMcpServer()
  const test = useTestMcpServer()
  const navigate = useNavigate()
  const [editing, setEditing] = useState(false)
  const [saving, setSaving] = useState(false)
  const [confirmOpen, setConfirmOpen] = useState(false)
  const saveReady = useEditSaveGuard(editing)

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

  return (
    <DetailShell
      title={s.name}
      subtitle={`${t(`mcp:transports.${TRANSPORT_KEYS[s.transport]}.label`)} · ${endpoint}`}
      actions={
        <>
          <Button
            variant="ghost"
            size="sm"
            disabled={test.isPending}
            onClick={() => test.mutate(s.id)}
          >
            {test.isPending ? <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" /> : null}
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
      <div className="space-y-8">
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
          <Field label={t('mcp:fields.enabled')}>
            <Badge variant={s.enabled ? 'default' : 'secondary'} className="mt-1">
              {s.enabled ? t('mcp:states.enabled') : t('mcp:states.disabled')}
            </Badge>
          </Field>
        </FieldGrid>

        {s.transport === 'stdio' && Object.keys(s.env).length > 0 ? (
          <Section title={t('mcp:fields.env')} as="h3">
            <ul className="space-y-1 font-mono text-xs">
              {Object.entries(s.env).map(([key, value]) => (
                <li key={key}>
                  {key}={value}
                </li>
              ))}
            </ul>
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
                <ul className="space-y-2">
                  {result.tools.map((tool) => (
                    <li
                      key={tool.exposed_name}
                      className="rounded-md border border-border bg-card p-3"
                    >
                      <code className="font-mono text-xs">{tool.exposed_name}</code>
                      {tool.description ? (
                        <p className="mt-1 text-xs text-muted-foreground">
                          {tool.description}
                        </p>
                      ) : null}
                    </li>
                  ))}
                </ul>
              )
            ) : (
              <p className="break-words text-sm text-destructive">{result.error}</p>
            )}
          </Section>
        ) : null}

        {s.description ? (
          <Section title={t('mcp:detail.description')} as="h3">
            <p className="whitespace-pre-wrap text-sm leading-relaxed">{s.description}</p>
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
