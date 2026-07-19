import { cleanup, render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { DirectChatPickerDialog } from './DirectChatPickerDialog'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  agents: vi.fn(),
  create: vi.fn(),
}))

vi.mock('@/hooks/useAgents', () => ({ useAgents: mocks.agents }))
vi.mock('@/hooks/useDirectChats', () => ({ useCreateDirectChat: mocks.create }))

describe('DirectChatPickerDialog', () => {
  beforeEach(() => {
    mocks.agents.mockReturnValue({
      data: [
        { id: 'agent-active', name: 'Planner', description: 'Plans launches', status: 'active' },
        { id: 'agent-disabled', name: 'Offline', description: null, status: 'disabled' },
      ],
      isLoading: false,
      error: null,
    })
    mocks.create.mockReturnValue({ isPending: false, mutateAsync: vi.fn() })
  })

  afterEach(cleanup)

  function renderPicker(onOpenChange = vi.fn()) {
    return render(
      <I18nextProvider i18n={i18n}>
        <MemoryRouter initialEntries={['/']}>
          <Routes>
            <Route path="/" element={<DirectChatPickerDialog open onOpenChange={onOpenChange} />} />
            <Route path="/chats/:chatId" element={<div>direct destination</div>} />
          </Routes>
        </MemoryRouter>
      </I18nextProvider>,
    )
  }

  it('searches only active Agents and navigates after creation', async () => {
    const user = userEvent.setup()
    const mutateAsync = vi.fn().mockResolvedValue({ id: 'chat-1' })
    mocks.create.mockReturnValue({ isPending: false, mutateAsync })
    const onOpenChange = vi.fn()
    renderPicker(onOpenChange)

    expect(screen.getByText('Planner')).toBeInTheDocument()
    expect(screen.queryByText('Offline')).not.toBeInTheDocument()
    await user.type(screen.getByRole('textbox', { name: 'Search Agents' }), 'plan')
    await user.click(screen.getByRole('button', { name: /Planner/ }))

    await waitFor(() => expect(mutateAsync).toHaveBeenCalledWith({ agent_id: 'agent-active' }))
    expect(onOpenChange).toHaveBeenCalledWith(false)
    expect(screen.getByText('direct destination')).toBeInTheDocument()
  })

  it('keeps the dialog open and shows the API error when creation fails', async () => {
    const user = userEvent.setup()
    mocks.create.mockReturnValue({
      isPending: false,
      mutateAsync: vi.fn().mockRejectedValue(new Error('agent unavailable')),
    })
    const onOpenChange = vi.fn()
    renderPicker(onOpenChange)

    await user.click(screen.getByRole('button', { name: /Planner/ }))

    expect(await screen.findByRole('alert')).toHaveTextContent('agent unavailable')
    expect(onOpenChange).not.toHaveBeenCalled()
    expect(screen.getByText('Start a direct chat')).toBeInTheDocument()
  })
})
