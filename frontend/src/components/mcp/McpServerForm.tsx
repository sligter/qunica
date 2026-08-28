import { useEffect, useMemo, useState } from 'react'
import { CheckCircle2, Loader2, XCircle } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { KeyValueEditor } from '@/components/mcp/KeyValueEditor'
import {
  maskedRowsFromRecord,
  recordFromRows,
  rowsFromRecord,
  secretRecordFromRows,
  type KeyValueRow,
} from '@/components/mcp/keyValueRows'
import { Button } from '@/components/ui/button'
import { FieldError, FormField } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { Switch } from '@/components/ui/switch'
import {
  useCreateMcpServer,
  useTestMcpDraft,
  useUpdateMcpServer,
} from '@/hooks/useMcpServers'
import { ApiError } from '@/lib/api-v2/client'
import { parseMcpConfig } from '@/lib/mcpConfigImport'
import type {
  McpServerCreate,
  McpServerRead,
  McpTestConnectionResult,
  McpTransport,
} from '@/types/api'

const TRANSPORTS: McpTransport[] = ['stdio', 'streamable-http', 'sse']

const TRANSPORT_KEYS: Record<McpTransport, 'stdio' | 'streamableHttp' | 'sse'> = {
  stdio: 'stdio',
  'streamable-http': 'streamableHttp',
  sse: 'sse',
}

/** Names the transport radiogroup for assistive technology. */
const TRANSPORT_LABEL_ID = 'mcp-transport-label'

/**
 * Arrow-key traversal inside the transport radiogroup.
 *
 * Matches what a native radio group does: one tab stop, arrows to choose, and
 * the selection moves as focus moves rather than requiring a second press.
 */
function transportStep(key: string, current: number): number {
  const forward = key === 'ArrowRight' || key === 'ArrowDown'
  const back = key === 'ArrowLeft' || key === 'ArrowUp'
  if (!forward && !back) return -1
  const delta = forward ? 1 : -1
  return (current + delta + TRANSPORTS.length) % TRANSPORTS.length
}

interface McpServerFormProps {
  /** The server being edited, or omitted to create a new one. */
  server?: McpServerRead
  onSaved?: (server: McpServerRead) => void
  onSavingChange?: (saving: boolean) => void
}

export const EDIT_MCP_SERVER_FORM_ID = 'edit-mcp-server-form'

/** Split a whitespace-separated argument line, honouring quoted segments. */
function parseArgs(raw: string): string[] {
  const matches = raw.match(/"[^"]*"|'[^']*'|\S+/g) ?? []
  return matches.map((token) => {
    const quoted =
      (token.startsWith('"') && token.endsWith('"')) ||
      (token.startsWith("'") && token.endsWith("'"))
    return quoted ? token.slice(1, -1) : token
  })
}

/** Re-quote arguments that contain spaces so a round trip is lossless. */
function formatArgs(args: string[]): string {
  return args.map((arg) => (arg.includes(' ') ? `"${arg}"` : arg)).join(' ')
}

/**
 * Create/edit form for one MCP server, with a live connection test.
 *
 * Header values arrive masked from the API, so the editor starts with empty
 * header values on an existing server: re-submitting the mask would store the
 * literal `****abcd` as the credential.
 */
export function McpServerForm({
  server,
  onSaved,
  onSavingChange,
}: McpServerFormProps) {
  const { t } = useTranslation(['mcp', 'common'])
  const create = useCreateMcpServer()
  const update = useUpdateMcpServer(server?.id)
  const testDraft = useTestMcpDraft()

  const [name, setName] = useState(server?.name ?? '')
  const [description, setDescription] = useState(server?.description ?? '')
  const [transport, setTransport] = useState<McpTransport>(server?.transport ?? 'stdio')
  const [command, setCommand] = useState(server?.command ?? '')
  const [argsText, setArgsText] = useState(formatArgs(server?.args ?? []))
  const [cwd, setCwd] = useState(server?.cwd ?? '')
  const [url, setUrl] = useState(server?.url ?? '')
  const [envRows, setEnvRows] = useState<KeyValueRow[]>(() => rowsFromRecord(server?.env))
  const [headerRows, setHeaderRows] = useState<KeyValueRow[]>(() =>
    maskedRowsFromRecord(server?.headers_masked),
  )
  const [timeoutSeconds, setTimeoutSeconds] = useState(server?.timeout_seconds ?? 60)
  const [toolFilterText, setToolFilterText] = useState((server?.tool_filter ?? []).join(', '))
  const [enabled, setEnabled] = useState(server?.enabled ?? true)
  const [jsonConfig, setJsonConfig] = useState('')
  const [jsonError, setJsonError] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [testResult, setTestResult] = useState<McpTestConnectionResult | null>(null)

  const isHttp = transport !== 'stdio'
  const saving = create.isPending || update.isPending

  // Moves focus with the selection, the way a native radio group behaves. The
  // buttons are keyed by transport so React keeps the same nodes across the
  // update, which is what lets the focus call survive the re-render.
  const onTransportKeys = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const next = transportStep(event.key, TRANSPORTS.indexOf(transport))
    if (next < 0) return
    event.preventDefault()
    setTransport(TRANSPORTS[next]!)
    event.currentTarget.querySelectorAll<HTMLElement>('[data-transport-option]')[next]?.focus()
  }

  useEffect(() => {
    onSavingChange?.(saving)
  }, [onSavingChange, saving])

  const payload = useMemo((): McpServerCreate => {
    return {
      name: name.trim(),
      description: description.trim() || null,
      transport,
      command: isHttp ? null : command.trim() || null,
      args: isHttp ? [] : parseArgs(argsText),
      env: isHttp ? {} : recordFromRows(envRows),
      cwd: isHttp ? null : cwd.trim() || null,
      url: isHttp ? url.trim() || null : null,
      // Header values are masked on the way out, so an untouched row sends
      // `null` ("keep what is stored") rather than the blank box the operator
      // sees. A row the operator deleted is absent from the map, which is how
      // the server is told to drop that header. Sending the rows verbatim would
      // overwrite every stored credential with an empty string.
      headers: isHttp ? secretRecordFromRows(headerRows) : {},
      timeout_seconds: timeoutSeconds,
      tool_filter: toolFilterText
        .split(',')
        .map((entry) => entry.trim())
        .filter(Boolean),
      enabled,
    }
  }, [
    argsText,
    command,
    cwd,
    description,
    enabled,
    envRows,
    headerRows,
    isHttp,
    name,
    timeoutSeconds,
    toolFilterText,
    transport,
    url,
  ])

  const errorText = (err: unknown): string =>
    err instanceof ApiError ? err.message : t('mcp:errors.network')

  const onApplyJson = () => {
    try {
      const config = parseMcpConfig(jsonConfig)
      if (config.name !== undefined) setName(config.name)
      if (config.description !== undefined) setDescription(config.description)
      setTransport(config.transport)
      setCommand(config.command ?? '')
      setArgsText(formatArgs(config.args))
      setEnvRows(rowsFromRecord(config.env))
      setCwd(config.cwd ?? '')
      setUrl(config.url ?? '')
      setHeaderRows(rowsFromRecord(config.headers).map((row) => ({ ...row, dirty: true })))
      if (config.timeoutSeconds !== undefined) setTimeoutSeconds(config.timeoutSeconds)
      if (config.toolFilter !== undefined) setToolFilterText(config.toolFilter.join(', '))
      if (config.enabled !== undefined) setEnabled(config.enabled)
      setJsonError(false)
    } catch {
      setJsonError(true)
    }
  }

  const onTest = async () => {
    setError(null)
    setTestResult(null)
    try {
      // Pass the row id so the server can resolve the "keep" header entries
      // against what is stored; without it the probe would dial with blanks and
      // report a failure the real configuration would not hit.
      setTestResult(
        await testDraft.mutateAsync(
          server ? { ...payload, server_id: server.id } : payload,
        ),
      )
    } catch (err) {
      setError(errorText(err))
    }
  }

  const onSubmit = async (event: React.FormEvent) => {
    event.preventDefault()
    setError(null)
    if (!payload.name) {
      setError(t('mcp:validation.nameRequired'))
      return
    }
    if (!isHttp && !payload.command) {
      setError(t('mcp:validation.commandRequired'))
      return
    }
    if (isHttp && !payload.url) {
      setError(t('mcp:validation.urlRequired'))
      return
    }
    try {
      const saved = server
        ? await update.mutateAsync(payload)
        : await create.mutateAsync(payload)
      onSaved?.(saved)
    } catch (err) {
      setError(errorText(err))
    }
  }

  return (
    <form
      id={server ? EDIT_MCP_SERVER_FORM_ID : undefined}
      onSubmit={(event) => void onSubmit(event)}
      className="space-y-5"
    >
      {!server ? (
        <div className="space-y-2 rounded-md border border-border bg-accent/20 p-3">
          <div>
            <Label htmlFor="mcp-json-config">{t('mcp:fields.jsonConfig')}</Label>
            <p className="mt-1 text-xs text-muted-foreground">
              {t('mcp:fields.jsonConfigHint')}
            </p>
          </div>
          <Textarea
            id="mcp-json-config"
            rows={5}
            value={jsonConfig}
            spellCheck={false}
            placeholder={'{\n  "command": "npx",\n  "args": ["chrome-devtools-mcp@latest"],\n  "type": "stdio"\n}'}
            className="font-mono text-xs"
            onChange={(event) => {
              setJsonConfig(event.target.value)
              setJsonError(false)
            }}
          />
          {jsonError ? (
            <p className="text-xs text-destructive" role="alert">
              {t('mcp:validation.invalidJsonConfig')}
            </p>
          ) : null}
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!jsonConfig.trim()}
            onClick={onApplyJson}
          >
            {t('mcp:actions.applyJsonConfig')}
          </Button>
        </div>
      ) : null}

      <FormField
        name="mcp-name"
        label={t('mcp:fields.name')}
        required
        description={t('mcp:fields.nameHint')}
        error={name.trim() ? undefined : t('common:validation.required')}
      >
        {(field) => (
          <Input
            {...field}
            value={name}
            placeholder={t('mcp:fields.namePlaceholder')}
            onChange={(event) => setName(event.target.value)}
          />
        )}
      </FormField>

      <div className="space-y-1.5">
        <Label htmlFor="mcp-description">{t('mcp:fields.description')}</Label>
        <Textarea
          id="mcp-description"
          rows={2}
          value={description}
          onChange={(event) => setDescription(event.target.value)}
        />
      </div>

      <div className="space-y-1.5">
        {/* A span, not a <label>: it names the group rather than one control.
            These cards are the transport picker, and without the radiogroup
            role a screen reader announced three unrelated buttons. */}
        <span id={TRANSPORT_LABEL_ID} className="text-sm font-medium leading-none">
          {t('mcp:fields.transport')}
        </span>
        <div
          role="radiogroup"
          aria-labelledby={TRANSPORT_LABEL_ID}
          className="grid gap-2 sm:grid-cols-3"
          onKeyDown={onTransportKeys}
        >
          {TRANSPORTS.map((option) => (
            <button
              key={option}
              type="button"
              role="radio"
              data-transport-option=""
              aria-checked={transport === option}
              // Roving tabindex: a radiogroup is one stop in the tab order, and
              // the arrow keys move within it. Three tabbable radios meant three
              // stops and no way to tell where the group began.
              tabIndex={transport === option ? 0 : -1}
              onClick={() => setTransport(option)}
              className={`rounded-md border p-3 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-1 focus-visible:ring-offset-background ${
                transport === option
                  ? 'border-primary bg-accent'
                  : 'border-border hover:bg-accent/50'
              }`}
            >
              <span className="block text-sm font-medium">
                {t(`mcp:transports.${TRANSPORT_KEYS[option]}.label`)}
              </span>
              <span className="mt-1 block text-xs text-muted-foreground">
                {t(`mcp:transports.${TRANSPORT_KEYS[option]}.hint`)}
              </span>
            </button>
          ))}
        </div>
      </div>

      {isHttp ? (
        <>
          <div className="space-y-1.5">
            <Label htmlFor="mcp-url">{t('mcp:fields.url')}</Label>
            <Input
              id="mcp-url"
              value={url}
              placeholder={
                transport === 'sse'
                  ? 'https://example.com/sse'
                  : 'https://example.com/mcp'
              }
              onChange={(event) => setUrl(event.target.value)}
            />
            <p className="text-xs text-muted-foreground">
              {t(`mcp:transports.${TRANSPORT_KEYS[transport]}.urlHint`)}
            </p>
          </div>
          <div className="space-y-1.5">
            <Label>{t('mcp:fields.headers')}</Label>
            <KeyValueEditor
              rows={headerRows}
              onChange={setHeaderRows}
              keyPlaceholder="Authorization"
              valuePlaceholder="Bearer ..."
              addLabel={t('mcp:actions.addHeader')}
              secret
              storedHints={server?.headers_masked}
            />
            <p className="text-xs text-muted-foreground">
              {server ? t('mcp:fields.headersMaskedHint') : t('mcp:fields.headersHint')}
            </p>
          </div>
        </>
      ) : (
        <>
          <div className="space-y-1.5">
            <Label htmlFor="mcp-command">{t('mcp:fields.command')}</Label>
            <Input
              id="mcp-command"
              value={command}
              placeholder="npx"
              onChange={(event) => setCommand(event.target.value)}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="mcp-args">{t('mcp:fields.args')}</Label>
            <Input
              id="mcp-args"
              value={argsText}
              placeholder="-y @modelcontextprotocol/server-filesystem ."
              onChange={(event) => setArgsText(event.target.value)}
            />
            <p className="text-xs text-muted-foreground">{t('mcp:fields.argsHint')}</p>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="mcp-cwd">{t('mcp:fields.cwd')}</Label>
            <Input
              id="mcp-cwd"
              value={cwd}
              placeholder="D:/projects/my-app"
              onChange={(event) => setCwd(event.target.value)}
            />
          </div>
          <div className="space-y-1.5">
            <Label>{t('mcp:fields.env')}</Label>
            <KeyValueEditor
              rows={envRows}
              onChange={setEnvRows}
              keyPlaceholder="API_KEY"
              valuePlaceholder="value"
              addLabel={t('mcp:actions.addEnv')}
            />
          </div>
        </>
      )}

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-1.5">
          <Label htmlFor="mcp-timeout">{t('mcp:fields.timeout')}</Label>
          <Input
            id="mcp-timeout"
            type="number"
            min={1}
            max={600}
            value={timeoutSeconds}
            onChange={(event) => setTimeoutSeconds(Number(event.target.value) || 60)}
          />
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="mcp-filter">{t('mcp:fields.toolFilter')}</Label>
          <Input
            id="mcp-filter"
            value={toolFilterText}
            placeholder={t('mcp:fields.toolFilterPlaceholder')}
            onChange={(event) => setToolFilterText(event.target.value)}
          />
          <p className="text-xs text-muted-foreground">{t('mcp:fields.toolFilterHint')}</p>
        </div>
      </div>

      <div className="flex items-center justify-between rounded-md border border-border p-3">
        <div className="min-w-0">
          <p className="text-sm font-medium">{t('mcp:fields.enabled')}</p>
          <p className="mt-1 text-xs text-muted-foreground">{t('mcp:fields.enabledHint')}</p>
        </div>
        <Switch checked={enabled} onCheckedChange={setEnabled} />
      </div>

      {testResult ? <TestResultPanel result={testResult} /> : null}

      {error ? <FieldError className="text-sm">{error}</FieldError> : null}

      <div className="flex items-center gap-2">
        {!server ? (
          <Button type="submit" disabled={saving}>
            {saving ? t('common:actions.saving') : t('mcp:actions.add')}
          </Button>
        ) : null}
        <Button
          type="button"
          variant="outline"
          disabled={testDraft.isPending}
          onClick={() => void onTest()}
        >
          {testDraft.isPending ? (
            <Loader2 className="mr-1 h-3.5 w-3.5 animate-spin" />
          ) : null}
          {t('mcp:actions.test')}
        </Button>
      </div>
    </form>
  )
}

/** Renders a connection probe: the discovered tools, or why the probe failed. */
function TestResultPanel({ result }: { result: McpTestConnectionResult }) {
  const { t } = useTranslation('mcp')

  if (!result.ok) {
    return (
      <div className="rounded-md border border-destructive/40 bg-destructive/5 p-3">
        <p className="flex items-center gap-2 text-sm font-medium text-destructive">
          <XCircle className="h-4 w-4" />
          {t('test.failed')}
        </p>
        <p className="mt-1 break-words text-xs text-muted-foreground">{result.error}</p>
      </div>
    )
  }

  return (
    <div className="rounded-md border border-border bg-accent/30 p-3">
      <p className="flex items-center gap-2 text-sm font-medium">
        <CheckCircle2 className="h-4 w-4 text-emerald-600" />
        {result.server_label
          ? t('test.connectedTo', { server: result.server_label })
          : t('test.connected')}
      </p>
      {result.tools.length === 0 ? (
        <p className="mt-1 text-xs text-muted-foreground">{t('test.noTools')}</p>
      ) : (
        <ul className="mt-2 space-y-1">
          {result.tools.map((tool) => (
            <li key={tool.exposed_name} className="text-xs">
              <code className="font-mono">{tool.exposed_name}</code>
              {tool.description ? (
                <span className="text-muted-foreground"> — {tool.description}</span>
              ) : null}
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
