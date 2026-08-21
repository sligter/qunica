import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { AgentSystemPromptActions } from '@/components/agents/AgentSystemPromptActions'
import i18n from '@/i18n'
import { useAuthStore } from '@/stores/authStore'

describe('AgentSystemPromptActions', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    useAuthStore.setState({ token: 'test-token' })
  })

  afterEach(() => {
    cleanup()
    vi.unstubAllGlobals()
    useAuthStore.setState({ token: null })
  })

  it('enhances the current prompt with the selected provider and model', async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({
      system_prompt: '# Reviewer\nReview carefully.',
    }), {
      status: 200,
      headers: { 'Content-Type': 'application/json' },
    }))
    vi.stubGlobal('fetch', fetchMock)
    const onApply = vi.fn()
    render(
      <QueryClientProvider client={new QueryClient()}>
        <AgentSystemPromptActions
          name="Reviewer"
          description="Reviews code"
          prompt="Be careful."
          providerId="provider-1"
          model="model-1"
          onApply={onApply}
        />
      </QueryClientProvider>,
    )

    expect(screen.getByRole('button', { name: 'Generate new' })).toBeEnabled()
    await userEvent.click(screen.getByRole('button', { name: 'Enhance' }))

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/agents/system-prompt/generate'),
      expect.objectContaining({
        body: JSON.stringify({
          name: 'Reviewer',
          description: 'Reviews code',
          system_prompt: 'Be careful.',
          llm_provider_id: 'provider-1',
          model: 'model-1',
        }),
      }),
    ))
    expect(onApply).toHaveBeenCalledWith('# Reviewer\nReview carefully.')
  })
})
