import { cleanup, render } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { GroupSettingsTab } from '@/pages/group/GroupSettingsTab'
import type { GroupRead } from '@/types/api'

/**
 * The alignment rules live in `SettingsRow`, but a rule only holds if every
 * caller actually leans on it. A row that keeps its own width silently opts out
 * and lands off the shared baseline again — which is exactly how these pages
 * drifted apart in the first place. So sweep the real rendered settings surface
 * rather than the primitive in isolation.
 */

const group: GroupRead = {
  id: 'group-1',
  workspace_id: 'workspace-1',
  name: 'Group one',
  description: null,
  announcement: null,
  free_speech: true,
  proactive_mode: false,
  allow_agent_free_mention: true,
  agent_free_mention_max_dispatches: 2,
  communication_mode: 'mesh',
  muted_agent_ids: null,
  admin_agent_ids: null,
  muted_member_ids: null,
  status: 'active',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
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

vi.mock('@/components/agents/WorkspaceField', () => ({
  WorkspaceField: () => <input aria-label="Workspace picker" readOnly />,
}))
vi.mock('@/pages/group/GroupSchedulerSettingsSection', () => ({
  GroupSchedulerSettingsSection: () => null,
}))
vi.mock('@/hooks/useGroups', () => ({
  useUpdateGroup: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))
vi.mock('@/hooks/useDeleteGroup', () => ({
  useDeleteGroup: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))
vi.mock('@/hooks/useGroupMessages', () => ({
  useClearGroupMessages: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))
vi.mock('@/terminal/TerminalRuntimeProvider', () => ({
  useTerminalRuntime: () => ({ closeConversation: vi.fn() }),
}))
vi.mock('react-router-dom', () => ({ useNavigate: () => vi.fn() }))

/** Widths a control must not set for itself — the column or section owns width. */
const STRAY_WIDTH = /(^|\s)(w-(16|20|28|32|36|40|44|48|56|64|72|80|96)|max-w-(xs|sm|md|lg|xl|2xl|3xl|4xl|5xl|6xl|none))(\s|$)/

describe('settings surfaces stay on one baseline', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('routes every inline control through the shared right-flushed column', () => {
    const { container } = render(<GroupSettingsTab group={group} />)

    const rows = [...container.querySelectorAll('[data-slot="settings-row"]')]
    const inline = rows.filter((row) => !row.hasAttribute('data-stacked'))
    expect(inline.length).toBeGreaterThan(0)

    for (const row of inline) {
      const control = row.querySelector('[data-slot="settings-control"]')
      if (!control) continue
      expect(control).toHaveClass('sm:w-72', 'sm:justify-end')
    }
  })

  it('leaves width to the column, so no control carries one of its own', () => {
    const { container } = render(<GroupSettingsTab group={group} />)

    const offenders = [...container.querySelectorAll('[data-slot="settings-row"] *')]
      .filter((node) => STRAY_WIDTH.test(node.className?.toString() ?? ''))
      .map((node) => `${node.tagName.toLowerCase()}.${node.className}`)

    expect(offenders).toEqual([])
  })

  it('lets every section run to the page edge, with no odd one out', () => {
    const { container } = render(<GroupSettingsTab group={group} />)

    const sections = [...container.querySelectorAll('section')]
    expect(sections.length).toBeGreaterThan(0)
    for (const section of sections) {
      // One section carrying its own cap is how the right edge went ragged
      // before: its rules stopped short while its neighbours' ran on.
      expect(section).toHaveClass('w-full')
      expect(section.className).not.toMatch(/\bmax-w-/)
    }
  })
})
