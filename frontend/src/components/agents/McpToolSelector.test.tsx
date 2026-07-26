import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { PropsWithChildren, ReactElement } from 'react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'

import { McpToolSelector } from '@/components/agents/McpToolSelector'
import { mergeToolConfig } from '@/components/agents/toolConfig'
import {
  maskedRowsFromRecord,
  secretRecordFromRows,
} from '@/components/mcp/keyValueRows'
import i18n from '@/i18n'
import type {
  AgentMcpServerSelection,
  BuiltinToolRead,
  McpServerRead,
  McpTestConnectionResult,
} from '@/types/api'

const mocks = vi.hoisted(() => ({
  toolsQuery: vi.fn(),
}))

vi.mock('@/hooks/useMcpServers', () => ({
  useMcpServerTools: (id: string | undefined, enabled: boolean) =>
    mocks.toolsQuery(id, enabled),
}))

function server(overrides: Partial<McpServerRead> = {}): McpServerRead {
  return {
    id: 'srv-a',
    name: 'GitHub',
    description: 'Issues and PRs',
    transport: 'stdio',
    slug: 'github',
    command: 'npx',
    args: [],
    env: {},
    cwd: null,
    url: null,
    headers_masked: {},
    timeout_seconds: 60,
    tool_filter: [],
    enabled: true,
    status: 'active',
    created_at: '2026-07-26T00:00:00Z',
    updated_at: '2026-07-26T00:00:00Z',
    ...overrides,
  }
}

function probeResult(): McpTestConnectionResult {
  return {
    ok: true,
    server_label: 'github-mcp@1.0.0',
    tools: [
      {
        name: 'create_issue',
        exposed_name: 'mcp__github__create_issue',
        description: 'Open an issue.',
      },
      { name: 'list_issues', exposed_name: 'mcp__github__list_issues', description: '' },
    ],
    error: null,
  }
}

function wrapper({ children }: PropsWithChildren): ReactElement {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  return (
    <MemoryRouter>
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    </MemoryRouter>
  )
}

beforeAll(async () => {
  // The assertions read user-facing copy, so pin the language rather than
  // letting the bootstrap detector pick one from the environment.
  await i18n.changeLanguage('en-US')
})

afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})


/** The server list, scoped so tool checkboxes cannot be picked up by mistake. */
function serverList() {
  return within(screen.getByRole('group', { name: 'MCP servers' }))
}

/** The nested per-tool list, which only exists while a server is expanded. */
function toolList() {
  return within(screen.getByRole('group', { name: 'MCP tools' }))
}

/** Expand the narrow-tools disclosure on the only server row. */
async function expandTools(label: string) {
  await userEvent.click(screen.getByRole('button', { name: new RegExp(label) }))
  await waitFor(() => expect(screen.getByRole('group', { name: 'MCP tools' })).toBeTruthy())
  return toolList().getAllByRole('checkbox') as HTMLInputElement[]
}

describe('McpToolSelector', () => {
  it('shows the tool prefix an agent would call, so the naming is not a surprise', () => {
    mocks.toolsQuery.mockReturnValue({ isLoading: false, error: null, data: undefined })

    render(<McpToolSelector servers={[server()]} value={[]} onChange={vi.fn()} />, {
      wrapper,
    })

    expect(screen.getByText('mcp__github__*')).toBeTruthy()
  })

  it('selecting a server exposes every tool until the operator narrows it', async () => {
    mocks.toolsQuery.mockReturnValue({ isLoading: false, error: null, data: undefined })
    const onChange = vi.fn()

    render(<McpToolSelector servers={[server()]} value={[]} onChange={onChange} />, {
      wrapper,
    })
    await userEvent.click(serverList().getByRole('checkbox'))

    expect(onChange).toHaveBeenCalledWith([
      { server_id: 'srv-a', enabled: true, tools: [] },
    ])
  })

  it('does not probe a server until its tool list is expanded', () => {
    mocks.toolsQuery.mockReturnValue({ isLoading: false, error: null, data: undefined })
    const selected: AgentMcpServerSelection[] = [
      { server_id: 'srv-a', enabled: true, tools: [] },
    ]

    render(<McpToolSelector servers={[server()]} value={selected} onChange={vi.fn()} />, {
      wrapper,
    })

    // Probing spawns a process for a stdio server, so a collapsed row must not
    // trigger one just because the server is selected.
    expect(mocks.toolsQuery).not.toHaveBeenCalled()
  })

  it('expanding a selected server lists its tools, all checked by default', async () => {
    mocks.toolsQuery.mockReturnValue({
      isLoading: false,
      error: null,
      data: probeResult(),
    })
    const selected: AgentMcpServerSelection[] = [
      { server_id: 'srv-a', enabled: true, tools: [] },
    ]

    render(<McpToolSelector servers={[server()]} value={selected} onChange={vi.fn()} />, {
      wrapper,
    })
    const boxes = await expandTools('All tools')

    expect(boxes).toHaveLength(2)
    // An empty selection means "all", so every box reads as checked rather than
    // presenting an all-empty list the operator would have to re-tick.
    expect(boxes.every((box) => box.checked)).toBe(true)
  })

  it('unchecking a tool narrows the selection to the remaining ones', async () => {
    mocks.toolsQuery.mockReturnValue({
      isLoading: false,
      error: null,
      data: probeResult(),
    })
    const onChange = vi.fn()
    const selected: AgentMcpServerSelection[] = [
      { server_id: 'srv-a', enabled: true, tools: ['create_issue', 'list_issues'] },
    ]

    render(
      <McpToolSelector servers={[server()]} value={selected} onChange={onChange} />,
      { wrapper },
    )
    const boxes = await expandTools('2 tools')
    await userEvent.click(boxes[1]!)

    expect(onChange).toHaveBeenCalledWith([
      { server_id: 'srv-a', enabled: true, tools: ['create_issue'] },
    ])
  })

  it('surfaces a failed probe instead of showing an empty tool list', async () => {
    mocks.toolsQuery.mockReturnValue({
      isLoading: false,
      error: null,
      data: { ok: false, server_label: null, tools: [], error: 'connection refused' },
    })
    const selected: AgentMcpServerSelection[] = [
      { server_id: 'srv-a', enabled: true, tools: [] },
    ]

    render(<McpToolSelector servers={[server()]} value={selected} onChange={vi.fn()} />, {
      wrapper,
    })
    await userEvent.click(screen.getByRole('button', { name: /All tools/ }))

    await waitFor(() => expect(screen.getByText('connection refused')).toBeTruthy())
  })

  it('points at the settings area when no server is configured', () => {
    mocks.toolsQuery.mockReturnValue({ isLoading: false, error: null, data: undefined })

    render(<McpToolSelector servers={[]} value={[]} onChange={vi.fn()} />, { wrapper })

    expect(screen.getByText('Configure an MCP server')).toBeTruthy()
  })
})

describe('mergeToolConfig', () => {
  const builtins: BuiltinToolRead[] = [
    {
      id: 'read',
      name: 'Read',
      description: 'Read files.',
      policy: 'read',
      requires_workspace: true,
      requires_sandbox: false,
      runtime_status: 'available',
    },
  ]

  it('carries MCP selections through a built-in catalog merge', () => {
    // The catalog covers built-ins only, so reconciling against it must not
    // drop a server selection the form is about to save.
    const merged = mergeToolConfig(builtins, {
      tools: { read: { enabled: true } },
      mcp_servers: [{ server_id: 'srv-a', enabled: true, tools: ['create_issue'] }],
    })

    expect(merged.mcp_servers).toEqual([
      { server_id: 'srv-a', enabled: true, tools: ['create_issue'] },
    ])
    expect(merged.tools.read?.enabled).toBe(true)
  })

  it('defaults an agent to no MCP servers rather than opting it into every one', () => {
    expect(mergeToolConfig(builtins, null).mcp_servers).toEqual([])
  })
})

describe('McpToolSelector tool narrowing regressions', () => {
  beforeEach(() => {
    mocks.toolsQuery.mockReturnValue({
      isLoading: false,
      error: null,
      data: probeResult(),
    })
  })

  const allSelected: AgentMcpServerSelection[] = [
    { server_id: 'srv-a', enabled: true, tools: [] },
  ]

  async function expandAllToolsRow() {
    return expandTools('All tools')
  }

  it('unchecking from the default state removes that tool instead of selecting it', async () => {
    // The empty array is the "all tools" sentinel. A plain membership toggle
    // would fall through to the add branch and narrow the agent down to exactly
    // the tool the operator was trying to take away.
    const onChange = vi.fn()
    render(
      <McpToolSelector servers={[server()]} value={allSelected} onChange={onChange} />,
      { wrapper },
    )

    const boxes = await expandAllToolsRow()
    await userEvent.click(boxes[0]!)

    expect(onChange).toHaveBeenCalledWith([
      { server_id: 'srv-a', enabled: true, tools: ['list_issues'] },
    ])
  })

  it('unchecking the last remaining tool deselects the server rather than re-granting all', async () => {
    // An empty list already means "all", so it cannot also mean "none".
    const onChange = vi.fn()
    const oneLeft: AgentMcpServerSelection[] = [
      { server_id: 'srv-a', enabled: true, tools: ['create_issue'] },
    ]
    render(
      <McpToolSelector servers={[server()]} value={oneLeft} onChange={onChange} />,
      { wrapper },
    )

    const boxes = await expandTools('1 tool')
    // The only checked box is create_issue; unchecking it leaves nothing.
    await userEvent.click(boxes[0]!)

    expect(onChange).toHaveBeenCalledWith([])
  })

  it('re-checking every tool collapses back to the all-tools sentinel', async () => {
    // Otherwise the frozen list would silently exclude any tool the server
    // gains later.
    const onChange = vi.fn()
    const oneSelected: AgentMcpServerSelection[] = [
      { server_id: 'srv-a', enabled: true, tools: ['create_issue'] },
    ]
    render(
      <McpToolSelector servers={[server()]} value={oneSelected} onChange={onChange} />,
      { wrapper },
    )

    const boxes = await expandTools('1 tool')
    await userEvent.click(boxes[1]!)

    expect(onChange).toHaveBeenCalledWith([
      { server_id: 'srv-a', enabled: true, tools: [] },
    ])
  })
})

describe('secretRecordFromRows', () => {
  it('sends null for a row the operator never typed into', () => {
    // Masked values never reach the client, so an untouched row must say "keep
    // what is stored" rather than submitting the blank box the operator sees.
    const rows = maskedRowsFromRecord({ Authorization: '****efgh' })

    expect(secretRecordFromRows(rows)).toEqual({ Authorization: null })
  })

  it('sends the new value once the row has been edited', () => {
    const rows = maskedRowsFromRecord({ Authorization: '****efgh' }).map((row) => ({
      ...row,
      value: 'Bearer new-token',
      dirty: true,
    }))

    expect(secretRecordFromRows(rows)).toEqual({ Authorization: 'Bearer new-token' })
  })

  it('omits a deleted row so the stored header is dropped', () => {
    // A key absent from the map is how a revoked credential gets deleted; if
    // deletion instead produced an empty map the caller could never revoke one.
    expect(secretRecordFromRows([])).toEqual({})
  })

  it('ignores rows whose key is still blank', () => {
    const rows = [{ id: 'r1', key: '   ', value: 'orphan', dirty: true }]

    expect(secretRecordFromRows(rows)).toEqual({})
  })
})
