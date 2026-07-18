import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, renderHook, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { PropsWithChildren, ReactElement } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { CreateAgentForm } from '@/components/agents/CreateAgentForm'
import { EditAgentForm } from '@/components/agents/EditAgentForm'
import type { AcpRuntimeCapabilitiesInput } from '@/hooks/useAcpRuntimeCapabilities'
import { useAuthStore } from '@/stores/authStore'
import type { AgentRead } from '@/types/api'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  capabilityHook: vi.fn(),
  capabilityRefetch: vi.fn(),
  capabilityState: {
    data: {
      models: [{ value: 'gpt-live', label: 'GPT Live', description: null }],
      modes: [{ value: 'auto', label: 'Auto', description: null }],
      thinking_efforts: [],
      current_model: 'gpt-live',
      current_mode: 'auto',
      current_thinking_effort: null,
      source: 'acp' as const,
      warning: 'Adapter warning',
    },
    isFetching: false,
    isError: false,
  },
  providerModelHook: vi.fn(),
  createMutate: vi.fn(),
  updateMutate: vi.fn(),
  providers: [
    {
      id: 'provider-1',
      name: 'Primary provider',
      kind: 'openai-compatible',
      base_url: null,
      api_key_masked: '***',
      default_model: 'provider-default',
      context_window_tokens: null,
      context_output_reserve_ratio: null,
      description: null,
      reasoning_passback: false,
      status: 'active',
      created_at: '2026-07-16T00:00:00Z',
    },
  ],
  providerModels: [{ id: 'provider-live', name: 'Provider Live' }],
  presets: [
    {
      id: 'codex',
      name: 'Codex',
      description: 'Codex ACP adapter.',
      profile: 'codex',
      installed: false,
      command: 'npx',
      args: ['-y', '@agentclientprotocol/codex-acp'],
      env: { ACP_MODE: 'test' },
      timeout_seconds: 3600,
      permission_policy: 'deny',
      default_model: 'gpt-preset',
      default_mode: 'agent',
      default_thinking_effort: 'xhigh',
      model_options: [],
      mode_options: [{ value: 'agent', label: 'Agent' }],
      thinking_effort_options: [{ value: 'xhigh', label: 'XHigh' }],
      install_hint: 'Install @agentclientprotocol/codex-acp.',
      source: 'fallback',
    },
  ],
  workspaces: [
    {
      id: 'workspace-1',
      name: 'Workspace',
      backend_type: 'local',
      local_path: 'D:/workspace',
      sandbox_ref: null,
      config: null,
      status: 'active',
      created_at: '2026-07-16T00:00:00Z',
      updated_at: '2026-07-16T00:00:00Z',
    },
  ],
}))

vi.mock('@/hooks/useAcpRuntimeCapabilities', () => ({
  acpRuntimeCapabilitiesQueryKey: (input: AcpRuntimeCapabilitiesInput | null) => [
    'agents',
    'acp-runtime-capabilities',
    JSON.stringify(input),
  ],
  useAcpRuntimeCapabilities: (input: AcpRuntimeCapabilitiesInput | null, enabled: boolean) => {
    mocks.capabilityHook(input, enabled)
    return {
      ...mocks.capabilityState,
      refetch: mocks.capabilityRefetch,
    }
  },
}))

vi.mock('@/hooks/useAcpRuntimePresets', () => ({
  useAcpRuntimePresets: () => ({ data: { presets: mocks.presets } }),
}))

vi.mock('@/hooks/useProviders', () => ({
  useProviders: () => ({ data: mocks.providers, isFetching: false, isError: false }),
  useProviderModels: (providerId: string | undefined) => {
    mocks.providerModelHook(providerId)
    return {
      data: providerId ? mocks.providerModels : [],
      isFetching: false,
      isError: false,
    }
  },
}))

vi.mock('@/hooks/useCreateAgent', () => ({
  useCreateAgent: () => ({ mutateAsync: mocks.createMutate, isPending: false }),
}))

vi.mock('@/hooks/useUpdateAgent', () => ({
  useUpdateAgent: () => ({ mutateAsync: mocks.updateMutate, isPending: false }),
}))

vi.mock('@/hooks/useSkills', () => ({
  useSkills: () => ({ data: [] }),
}))

vi.mock('@/hooks/useWorkspaces', () => ({
  useWorkspaces: () => ({ data: mocks.workspaces }),
  useCreateWorkspace: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))

vi.mock('@/hooks/useBuiltinTools', () => ({
  useBuiltinTools: () => ({ data: { tools: [] }, isLoading: false }),
}))

vi.mock('@/hooks/useAgents', () => ({
  useAgents: () => ({ data: [] }),
}))

vi.mock('@/hooks/useGroups', () => ({
  useGroups: () => ({ data: [] }),
}))

function agent(overrides: Partial<AgentRead>): AgentRead {
  return {
    id: 'agent-1',
    name: 'Runtime agent',
    description: null,
    system_prompt: 'You are helpful.',
    llm_config: null,
    tool_config: null,
    runtime_kind: 'llm_chat',
    acp_runtime: null,
    workspace_id: 'workspace-1',
    llm_provider_id: null,
    skill_ids: [],
    visibility: 'private',
    status: 'active',
    created_at: '2026-07-16T00:00:00Z',
    ...overrides,
  }
}

function lastCapabilityInput(): AcpRuntimeCapabilitiesInput | undefined {
  const inputs = mocks.capabilityHook.mock.calls
    .map(([input]) => input as AcpRuntimeCapabilitiesInput | null)
    .filter((input): input is AcpRuntimeCapabilitiesInput => input !== null)
  return inputs[inputs.length - 1]
}

function renderForm(element: ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
  })
  return render(
    <QueryClientProvider client={queryClient}>{element}</QueryClientProvider>,
  )
}

describe('agent runtime capabilities', () => {
  beforeEach(() => {
    mocks.capabilityHook.mockReset()
    mocks.capabilityRefetch.mockReset()
    mocks.capabilityRefetch.mockResolvedValue(undefined)
    mocks.providerModelHook.mockReset()
    mocks.createMutate.mockReset()
    mocks.updateMutate.mockReset()
    mocks.updateMutate.mockResolvedValue(undefined)
    mocks.capabilityState.isFetching = false
    mocks.capabilityState.isError = false
    mocks.capabilityState.data.warning = 'Adapter warning'
  })

  afterEach(async () => {
    cleanup()
    useAuthStore.setState({ token: null })
    vi.unstubAllGlobals()
    await i18n.changeLanguage('en-US')
  })

  it('renders translated agent fields, runtime choices, and validation errors', async () => {
    await i18n.changeLanguage('zh-CN')
    const user = userEvent.setup()
    renderForm(<CreateAgentForm />)

    expect(screen.getByLabelText('名称')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /LLM 对话提供商原生模型和工具/ })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: /^ACPAgent Client Protocol 进程$/ })).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '创建 Agent' }))

    expect(await screen.findByText('Agent 名称为必填项。')).toBeInTheDocument()
    expect(screen.getByText('工作区为必填项。')).toBeInTheDocument()
  })

  it('auto-probes ACP on open and model commit while keeping command edits stale until refresh', async () => {
    const user = userEvent.setup()
    renderForm(
      <EditAgentForm
        agent={agent({
          runtime_kind: 'acp',
          acp_runtime: {
            profile: 'custom',
            command: 'old-adapter',
            args: ['serve'],
            env: { TOKEN: 'secret' },
            permission_policy: 'deny',
            model: 'saved-custom',
          },
        })}
      />,
    )

    await waitFor(() => expect(lastCapabilityInput()?.command).toBe('old-adapter'))
    expect(lastCapabilityInput()).toEqual({
      profile: 'custom',
      command: 'old-adapter',
      args: ['serve'],
      env: { TOKEN: 'secret' },
      permission_policy: 'deny',
      selected_model: 'saved-custom',
    })
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('saved-custom')
    expect(screen.getByRole('status')).toHaveTextContent('Adapter warning')

    await user.type(screen.getByLabelText('Command'), '-edited')
    expect(screen.getByRole('status')).toHaveTextContent(
      'Runtime settings changed. Refresh available values.',
    )
    expect(lastCapabilityInput()?.command).toBe('old-adapter')

    const model = screen.getByRole('combobox', { name: 'Model' })
    await user.clear(model)
    await user.type(model, 'custom-next')
    expect(lastCapabilityInput()?.selected_model).toBe('saved-custom')
    await user.tab()

    await waitFor(() => expect(lastCapabilityInput()?.selected_model).toBe('custom-next'))
    expect(lastCapabilityInput()?.command).toBe('old-adapter')

    await user.click(screen.getByRole('button', { name: 'Refresh available values' }))
    await waitFor(() => expect(lastCapabilityInput()?.command).toBe('old-adapter-edited'))
    expect(lastCapabilityInput()?.selected_model).toBe('custom-next')

    await user.click(screen.getByRole('button', { name: 'Refresh available values' }))
    await waitFor(() => expect(mocks.capabilityRefetch).toHaveBeenCalled())
  })

  it('commits preset runtime settings and keeps preset choices as category fallbacks', async () => {
    const user = userEvent.setup()
    renderForm(<CreateAgentForm />)

    await user.click(screen.getByRole('button', { name: /^ACPAgent Client Protocol process$/ }))
    await user.click(screen.getByRole('button', { name: /CodexUses fallback command/ }))

    await waitFor(() => expect(lastCapabilityInput()?.profile).toBe('codex'))
    expect(lastCapabilityInput()).toEqual({
      profile: 'codex',
      command: 'npx',
      args: ['-y', '@agentclientprotocol/codex-acp'],
      env: { ACP_MODE: 'test' },
      permission_policy: 'deny',
      selected_model: 'gpt-preset',
    })
    expect(screen.getByRole('combobox', { name: 'Model' })).toHaveValue('gpt-preset')
    await user.click(screen.getByRole('button', { name: 'Show thinking options' }))
    expect(screen.getByRole('option', { name: /xhigh/i })).toHaveTextContent('XHigh')

    const mode = screen.getByRole('combobox', { name: 'Mode' })
    await user.clear(mode)
    await user.type(mode, 'custom-mode')
    expect(mode).toHaveValue('custom-mode')
  })

  it('loads provider suggestions without replacing or rejecting a custom model override', async () => {
    const user = userEvent.setup()
    renderForm(
      <EditAgentForm
        agent={agent({
          runtime_kind: 'llm_chat',
          llm_provider_id: 'provider-1',
          llm_config: { model: 'saved-provider-custom' },
        })}
      />,
    )

    expect(mocks.providerModelHook).toHaveBeenCalledWith('provider-1')
    const model = screen.getByRole('combobox', { name: 'Model' })
    expect(model).toHaveValue('saved-provider-custom')
    await user.click(screen.getByRole('button', { name: 'Show model options' }))
    expect(screen.getByRole('option', { name: /provider-live/i })).toHaveTextContent(
      'Provider Live',
    )

    await user.clear(model)
    await user.type(model, 'private-provider-model')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(mocks.updateMutate).toHaveBeenCalledOnce())
    expect(mocks.updateMutate).toHaveBeenCalledWith(
      expect.objectContaining({
        runtime_kind: 'llm_chat',
        llm_provider_id: 'provider-1',
        llm_config: expect.objectContaining({ model: 'private-provider-model' }),
      }),
    )
  })

  it('persists a custom provider model when creating a non-ACP agent', async () => {
    const user = userEvent.setup()
    mocks.createMutate.mockResolvedValue(
      agent({ id: 'created-agent', name: 'Created agent' }),
    )
    renderForm(<CreateAgentForm />)

    await user.type(screen.getByLabelText('Name'), 'Created agent')
    await user.selectOptions(screen.getByLabelText('Workspace'), 'workspace-1')
    await user.selectOptions(screen.getByLabelText('LLM provider'), 'provider-1')
    const model = screen.getByRole('combobox', { name: 'Model' })
    await user.type(model, 'create-only-model')
    await user.click(screen.getByRole('button', { name: 'Create agent' }))

    await waitFor(() => expect(mocks.createMutate).toHaveBeenCalledOnce())
    expect(mocks.createMutate).toHaveBeenCalledWith(
      expect.objectContaining({
        runtime_kind: 'llm_chat',
        llm_provider_id: 'provider-1',
        llm_config: expect.objectContaining({ model: 'create-only-model' }),
      }),
    )
  })

  it('posts the flat backend capability shape and keeps results fresh for five minutes', async () => {
    const actual = await vi.importActual<
      typeof import('@/hooks/useAcpRuntimeCapabilities')
    >('@/hooks/useAcpRuntimeCapabilities')
    const fetchMock = vi.fn().mockResolvedValue(
      new Response(
        JSON.stringify({
          models: [],
          modes: [],
          thinking_efforts: [],
          current_model: 'gpt-flat',
          current_mode: null,
          current_thinking_effort: null,
          source: 'acp',
          warning: null,
        }),
        { status: 200, headers: { 'Content-Type': 'application/json' } },
      ),
    )
    vi.stubGlobal('fetch', fetchMock)
    useAuthStore.setState({ token: 'owner-token' })
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    })
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )

    const { result } = renderHook(
      () =>
        actual.useAcpRuntimeCapabilities(
          {
            profile: 'custom',
            command: 'adapter',
            args: ['serve'],
            env: { MODE: 'test' },
            permission_policy: 'deny',
            selected_model: 'gpt-flat',
          },
          true,
        ),
      { wrapper },
    )

    await waitFor(() => expect(result.current.isSuccess).toBe(true))
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit]
    expect(JSON.parse(String(init.body))).toEqual({
      profile: 'custom',
      command: 'adapter',
      args: ['serve'],
      env: { MODE: 'test' },
      permission_policy: 'deny',
      model: 'gpt-flat',
    })
    expect(JSON.parse(String(init.body))).not.toHaveProperty('selected_model')
    const cachedOptions = queryClient.getQueryCache().getAll()[0]?.options as {
      staleTime?: number
    }
    expect(cachedOptions.staleTime).toBe(5 * 60 * 1000)
  })
})
