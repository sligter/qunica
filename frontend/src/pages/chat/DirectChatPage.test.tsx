import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { I18nextProvider } from 'react-i18next'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'

import { DirectChatPage } from './DirectChatPage'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({ useDirectChat: vi.fn() }))
vi.mock('@/hooks/useDirectChats', () => ({
  useDirectChat: mocks.useDirectChat,
  directChatQueryKey: (id: string) => ['direct-chats', id],
  directChatsQueryKey: ['direct-chats'],
  replaceDirectChatInList: (items: unknown) => items,
  useRenameDirectChat: () => ({ mutateAsync: vi.fn() }),
}))
vi.mock('@/components/chat/ConversationChatView', () => ({
  ConversationChatView: ({ disabledComposerReason, capabilities, agents }: { disabledComposerReason?: string; capabilities: { showManage: boolean; showTurnTrace: boolean; allowMentions: boolean }; agents: Array<{ display_name: string }> }) => <div>agent:{agents[0]?.display_name ?? 'none'} disabled:{disabledComposerReason ?? 'no'} manage:{String(capabilities.showManage)} trace:{String(capabilities.showTurnTrace)} mentions:{String(capabilities.allowMentions)}</div>,
}))

describe('DirectChatPage', () => {
  it('shows the sole Agent and disables composition when unavailable', () => {
    mocks.useDirectChat.mockReturnValue({ isLoading: false, error: null, data: { id: 'chat-1', title: 'Direct', title_source: 'automatic', agent_id: 'agent-1', agent_name: 'Solo', agent_status: 'deleted', workspace_id: null, status: 'active', created_at: '2026-07-19T00:00:00Z', updated_at: '2026-07-19T00:00:00Z' } })
    render(
      <I18nextProvider i18n={i18n}>
        <QueryClientProvider client={new QueryClient()}>
          <MemoryRouter initialEntries={['/chats/chat-1']}><Routes><Route path="/chats/:chatId" element={<DirectChatPage />} /></Routes></MemoryRouter>
        </QueryClientProvider>
      </I18nextProvider>,
    )
    expect(screen.getByText(/agent:Solo/)).toHaveTextContent('disabled:This Agent is unavailable. History remains readable.')
    expect(screen.getByText(/agent:Solo/)).toHaveTextContent('manage:false trace:false mentions:false')
  })
})
