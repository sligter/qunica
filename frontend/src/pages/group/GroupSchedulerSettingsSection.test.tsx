import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { ApiError } from '@/lib/api-v2/client'
import i18n from '@/i18n'
import { GroupSchedulerSettingsSection } from '@/pages/group/GroupSchedulerSettingsSection'
import type { GroupRead } from '@/types/api'

class ResizeObserverMock {
  observe() {}
  unobserve() {}
  disconnect() {}
}

vi.stubGlobal('ResizeObserver', ResizeObserverMock)
Element.prototype.hasPointerCapture = () => false
Element.prototype.setPointerCapture = () => {}
Element.prototype.releasePointerCapture = () => {}
Element.prototype.scrollIntoView = () => {}

const mocks = vi.hoisted(() => ({
  mutateAsync: vi.fn(),
  updateState: {
    isPending: false,
    error: null,
    mutateAsync: vi.fn(),
  },
  providers: [
    {
      id: 'provider-1',
      name: 'Primary provider',
      kind: 'openai-compatible',
      base_url: null,
      api_key_masked: '***',
      default_model: 'gpt-test',
      context_window_tokens: null,
      context_output_reserve_ratio: null,
      description: null,
      reasoning_passback: false,
      models: [{
        id: 'gpt-test',
        context_window_tokens: null,
        context_output_reserve_ratio: null,
      }],
      status: 'active',
      created_at: '2026-01-01T00:00:00Z',
    },
    {
      id: 'provider-2',
      name: 'Secondary provider',
      kind: 'openai-compatible',
      base_url: null,
      api_key_masked: '***',
      default_model: 'gpt-secondary',
      context_window_tokens: null,
      context_output_reserve_ratio: null,
      description: null,
      reasoning_passback: false,
      models: [{
        id: 'gpt-secondary',
        context_window_tokens: null,
        context_output_reserve_ratio: null,
      }],
      status: 'active',
      created_at: '2026-01-01T00:00:00Z',
    },
  ],
  modelsByProvider: {
    'provider-1': [{ id: 'gpt-test', name: 'GPT test' }],
    'provider-2': [{ id: 'gpt-secondary', name: 'GPT secondary' }],
  } as Record<string, Array<{ id: string; name: string }>>,
}))

vi.mock('@/hooks/useGroups', () => ({
  useUpdateGroup: () => mocks.updateState,
}))

vi.mock('@/hooks/useProviders', () => ({
  useProviders: () => ({ data: mocks.providers, isLoading: false }),
  useProviderModels: (providerId: string | undefined) => ({
    data: providerId ? mocks.modelsByProvider[providerId] ?? [] : [],
    isLoading: false,
  }),
}))

const group: GroupRead = {
  id: 'group-1',
  workspace_id: null,
  name: 'Operations',
  description: null,
  announcement: null,
  free_speech: false,
  proactive_mode: false,
  allow_agent_free_mention: false,
  agent_free_mention_max_dispatches: 0,
  communication_mode: 'mesh',
  muted_agent_ids: null,
  admin_agent_ids: null,
  muted_member_ids: null,
  status: 'active',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
  scheduler_mode: 'bounded',
  agent_mention_policy: 'display_only',
  max_agent_steps: null,
  max_steps_per_agent: 3,
  max_scheduler_hops: 5,
  max_moderator_calls: 4,
  max_consecutive_failures: 3,
  max_total_failures: 6,
  max_total_tokens: 120000,
  turn_timeout_seconds: 300,
  moderator_enabled: false,
  moderator_provider_id: null,
  moderator_model: null,
}

function renderSection(nextGroup: GroupRead = group) {
  return render(
    <MemoryRouter>
      <GroupSchedulerSettingsSection group={nextGroup} />
    </MemoryRouter>,
  )
}

describe('GroupSchedulerSettingsSection', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  beforeEach(() => {
    mocks.mutateAsync.mockReset()
    mocks.updateState.isPending = false
    mocks.updateState.error = null
    mocks.updateState.mutateAsync = mocks.mutateAsync
    mocks.mutateAsync.mockImplementation(async (payload: Partial<GroupRead>) => ({
      ...group,
      ...payload,
    }))
  })

  it('localizes scheduler labels and empty provider state', async () => {
    await i18n.changeLanguage('en-US')
    renderSection()
    expect(screen.getByText(i18n.t('groups:scheduler.title'))).toBeVisible()
    expect(screen.queryByRole('combobox', { name: 'Agent mention policy' })).toBeNull()
    expect(screen.getAllByText('No provider')[0]).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    renderSection()
    expect(screen.getByText(i18n.t('groups:scheduler.title'))).toBeVisible()
    expect(screen.getAllByText(i18n.t('groups:scheduler.noProvider'))[0]).toBeVisible()
  })

  it('renders null max_agent_steps as the automatic budget and submits null', async () => {
    const user = userEvent.setup()
    renderSection()

    expect(screen.getByRole('combobox', { name: 'Maximum agent steps mode' })).toHaveTextContent(
      'Automatic budget',
    )

    await user.clear(screen.getByRole('spinbutton', { name: 'Steps per agent' }))
    await user.type(screen.getByRole('spinbutton', { name: 'Steps per agent' }), '4')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(mocks.mutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({ max_agent_steps: null }),
      )
    })
  })

  it('submits the complete scheduler configuration atomically', async () => {
    const user = userEvent.setup()
    renderSection()

    await user.clear(screen.getByRole('spinbutton', { name: 'Steps per agent' }))
    await user.type(screen.getByRole('spinbutton', { name: 'Steps per agent' }), '4')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(mocks.mutateAsync).toHaveBeenCalledTimes(1)
    })
    expect(mocks.mutateAsync).toHaveBeenCalledWith({
      scheduler_mode: 'bounded',
      max_agent_steps: null,
      max_steps_per_agent: 4,
      max_scheduler_hops: 5,
      max_moderator_calls: 4,
      max_consecutive_failures: 3,
      max_total_failures: 6,
      max_total_tokens: 120000,
      turn_timeout_seconds: 300,
      moderator_enabled: false,
      moderator_provider_id: null,
      moderator_model: null,
    })
  })

  it('enables automatic scheduling and disables only ignored work limits', async () => {
    const user = userEvent.setup()
    renderSection({
      ...group,
      moderator_enabled: true,
      moderator_provider_id: 'provider-1',
      moderator_model: 'gpt-test',
    })

    await user.click(screen.getByRole('combobox', { name: 'Turn style' }))
    await user.click(await screen.findByRole('option', { name: 'Moderated discussion' }))

    expect(screen.getByRole('switch', { name: 'Enable moderator' })).toBeDisabled()
    expect(screen.getByLabelText('Steps per agent')).toBeDisabled()
    expect(screen.getByLabelText('Total tokens')).toBeDisabled()
    expect(screen.getByLabelText('Consecutive failures')).toBeEnabled()
    expect(screen.getByLabelText('Moderator timeout')).toBeEnabled()

    await user.click(screen.getByRole('button', { name: 'Save' }))
    await waitFor(() => {
      expect(mocks.mutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({ scheduler_mode: 'automatic', moderator_enabled: true }),
      )
    })
  })

  it('does not overwrite the follow-up policy while editing scheduler budgets', async () => {
    const user = userEvent.setup()
    renderSection({
      ...group,
      agent_mention_policy: 'bounded_schedule',
    } as unknown as GroupRead)

    expect(screen.queryByRole('combobox', { name: 'Agent mention policy' })).toBeNull()
    await user.clear(screen.getByRole('spinbutton', { name: 'Steps per agent' }))
    await user.type(screen.getByRole('spinbutton', { name: 'Steps per agent' }), '4')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => expect(mocks.mutateAsync).toHaveBeenCalledTimes(1))
    expect(mocks.mutateAsync.mock.calls[0]?.[0]).not.toHaveProperty('agent_mention_policy')
  })

  it('requires an active provider and model when the moderator is enabled', async () => {
    const user = userEvent.setup()
    renderSection()

    await user.click(screen.getByRole('switch', { name: 'Enable moderator' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Choose an active provider for the moderator')).toBeVisible()
    expect(await screen.findByText('Choose a model for the moderator')).toBeVisible()
    expect(mocks.mutateAsync).not.toHaveBeenCalled()
  })

  it('clears a previous-provider model and requires a valid replacement before save', async () => {
    const user = userEvent.setup()
    renderSection({
      ...group,
      moderator_enabled: true,
      moderator_provider_id: 'provider-1',
      moderator_model: 'gpt-test',
    })

    expect(screen.getByRole('combobox', { name: 'Moderator model' })).toHaveTextContent(
      'GPT test',
    )
    await user.click(screen.getByRole('combobox', { name: 'Moderator provider' }))
    await user.click(
      await screen.findByRole('option', { name: 'Secondary provider - gpt-secondary' }),
    )

    expect(screen.getByRole('combobox', { name: 'Moderator model' })).toHaveTextContent(
      'No model',
    )
    await user.click(screen.getByRole('button', { name: 'Save' }))
    expect(await screen.findByText('Choose a model for the moderator')).toBeVisible()
    expect(mocks.mutateAsync).not.toHaveBeenCalled()

    await user.click(screen.getByRole('combobox', { name: 'Moderator model' }))
    expect(screen.queryByRole('option', { name: 'GPT test' })).not.toBeInTheDocument()
    await user.click(await screen.findByRole('option', { name: 'GPT secondary' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))

    await waitFor(() => {
      expect(mocks.mutateAsync).toHaveBeenCalledWith(
        expect.objectContaining({
          moderator_provider_id: 'provider-2',
          moderator_model: 'gpt-secondary',
        }),
      )
    })
  })

  it('does not mutate when numeric input is outside backend bounds', async () => {
    const user = userEvent.setup()
    renderSection()

    await user.clear(screen.getByLabelText('Steps per agent'))
    await user.type(screen.getByLabelText('Steps per agent'), '0')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByText('Must be at least 1')).toBeVisible()
    expect(mocks.mutateAsync).not.toHaveBeenCalled()
  })

  it('links to group members after a topology validation error', async () => {
    const user = userEvent.setup()
    mocks.mutateAsync.mockRejectedValueOnce(
      new ApiError(422, 'invalid_input', 'star topology has no hub'),
    )
    renderSection()

    await user.clear(screen.getByLabelText('Steps per agent'))
    await user.type(screen.getByLabelText('Steps per agent'), '4')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    const link = await screen.findByRole('link', { name: 'Review group members' })
    expect(link).toHaveAttribute('href', '/groups/group-1/manage?tab=members')
  })

  it('shows localized framing for an unexpected scheduler update error', async () => {
    const user = userEvent.setup()
    mocks.mutateAsync.mockRejectedValueOnce(new Error('offline'))
    renderSection()

    await user.clear(screen.getByLabelText('Steps per agent'))
    await user.type(screen.getByLabelText('Steps per agent'), '4')
    await user.click(screen.getByRole('button', { name: 'Save' }))

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to update scheduler settings',
    )
  })

  it('enables Save only when dirty and disables it while pending', async () => {
    const user = userEvent.setup()
    const view = renderSection()
    const save = screen.getByRole('button', { name: 'Save' })

    expect(save).toBeDisabled()
    await user.clear(screen.getByLabelText('Steps per agent'))
    await user.type(screen.getByLabelText('Steps per agent'), '4')
    expect(save).toBeEnabled()

    mocks.updateState.isPending = true
    view.rerender(
      <MemoryRouter>
        <GroupSchedulerSettingsSection group={group} />
      </MemoryRouter>,
    )

    expect(screen.getByRole('button', { name: 'Saving…' })).toBeDisabled()
  })

  it('does not overwrite dirty edits when the group prop refreshes', async () => {
    const user = userEvent.setup()
    const view = renderSection()

    await user.clear(screen.getByLabelText('Total tokens'))
    await user.type(screen.getByLabelText('Total tokens'), '120001')
    view.rerender(
      <MemoryRouter>
        <GroupSchedulerSettingsSection group={{ ...group, max_total_tokens: 999 }} />
      </MemoryRouter>,
    )

    expect(screen.getByLabelText('Total tokens')).toHaveValue(120001)
  })
})
