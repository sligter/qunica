import { render, screen } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { MemoryRouter } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { ChatHomePage } from './ChatHomePage'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({ groups: vi.fn(), directChats: vi.fn() }))
vi.mock('@/hooks/useGroups', () => ({ useGroups: mocks.groups }))
vi.mock('@/hooks/useDirectChats', () => ({ useDirectChats: mocks.directChats }))
vi.mock('@/hooks/useGroupMessages', () => ({ useConversationPrefetch: () => vi.fn() }))
vi.mock('@/components/groups/GroupFormDialog', () => ({ GroupFormDialog: () => null }))
vi.mock('@/components/direct-chats/DirectChatPickerDialog', () => ({ DirectChatPickerDialog: () => null }))

describe('ChatHomePage', () => {
  beforeEach(() => {
    mocks.groups.mockReturnValue({ isLoading: false, error: null, data: [{ id: 'group-1', name: 'Older group', description: null, avatar_url: null, avatar_members: [{ id: 'user-1', name: 'Alice', kind: 'user', avatar_url: null }, { id: 'agent-1', name: 'Builder', kind: 'agent', avatar_url: 'preset:loom' }], created_at: '2026-07-18T00:00:00Z', updated_at: '2026-07-18T00:00:00Z' }] })
    mocks.directChats.mockReturnValue({ isLoading: false, error: null, data: [{ id: 'chat-1', title: 'Newest direct', agent_name: 'Solo', updated_at: '2026-07-20T00:00:00Z' }] })
  })

  it('mixes recent direct and group conversations by activity', () => {
    render(<I18nextProvider i18n={i18n}><MemoryRouter><ChatHomePage /></MemoryRouter></I18nextProvider>)
    expect(screen.getByRole('button', { name: 'New chat' })).toBeInTheDocument()
    expect(screen.getByRole('button', { name: 'New group' })).toBeInTheDocument()
    const links = screen.getAllByRole('link')
    expect(links.map((link) => link.getAttribute('href'))).toEqual(['/chats/chat-1', '/groups/group-1'])
    expect(
      screen.getByText('Older group').closest('a')?.querySelector('[data-slot="group-avatar"]'),
    ).toHaveAttribute('data-member-count', '2')
  })
})
