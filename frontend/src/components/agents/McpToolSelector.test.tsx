import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { PropsWithChildren, ReactElement } from 'react'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest'

import { McpToolSelector } from '@/components/agents/McpToolSelector'
import { mergeToolConfig } from '@/components/agents/toolConfig'
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
    await userEvent.click(screen.getByRole('button', { pressed: false }))

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
    await userEvent.click(screen.getByText('All tools'))

    await waitFor(() => expect(screen.getByText('create_issue')).toBeTruthy())
    const boxes = screen.getAllByRole('checkbox') as HTMLInputElement[]
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
    await userEvent.click(screen.getByText('2 tools'))
    await waitFor(() => expect(screen.getByText('list_issues')).toBeTruthy())
    const boxes = screen.getAllByRole('checkbox')
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
    await userEvent.click(screen.getByText('All tools'))

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
