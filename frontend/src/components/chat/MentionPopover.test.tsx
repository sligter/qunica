import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { MentionPopover } from '@/components/chat/MentionPopover'
import i18n from '@/i18n'
import type { GroupAgentRead } from '@/types/api'

const agent: GroupAgentRead = {
  id: 'agent-raw-id',
  group_id: 'group-1',
  agent_id: 'agent-definition-1',
  display_name: 'Agent_RAW_原文',
  role: null,
  topology_role: null,
  speaking_order: null,
  response_mode: 'normal',
  share_group_workspace: false,
  context_usage: null,
  status: 'idle',
  joined_at: '2026-07-18T00:00:00Z',
}

describe('MentionPopover i18n', () => {
  beforeEach(async () => {
    await i18n.changeLanguage('en-US')
    Object.defineProperty(HTMLElement.prototype, 'scrollIntoView', {
      configurable: true,
      value: vi.fn(),
    })
  })
  afterEach(cleanup)

  it('uses an English accessible picker label and preserves the agent name', () => {
    render(<MentionPopover agents={[agent]} query="" visible onSelect={vi.fn()} onClose={vi.fn()} />)
    expect(screen.getByRole('listbox', { name: 'Mention an agent' })).toBeVisible()
    expect(screen.getByRole('option', { name: 'Agent_RAW_原文' })).toBeVisible()
  })

  it('uses a Chinese accessible picker label and preserves the agent name', async () => {
    await i18n.changeLanguage('zh-CN')
    render(<MentionPopover agents={[agent]} query="" visible onSelect={vi.fn()} onClose={vi.fn()} />)
    expect(screen.getByRole('listbox', { name: '提及 Agent' })).toBeVisible()
    expect(screen.getByRole('option', { name: 'Agent_RAW_原文' })).toBeVisible()
  })
})
