import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import '@/i18n'
import { GroupDecisionSettingsSection } from '@/pages/group/GroupDecisionSettingsSection'
import type { GroupRead } from '@/types/api'

vi.mock('@/hooks/useGroups', () => ({
  useUpdateGroup: () => ({ isPending: false, mutateAsync: vi.fn() }),
}))

const group = {
  id: 'group-1',
  scheduler_mode: 'bounded',
  decision_enabled: true,
  decision_scenarios: {
    moderator_selection: false,
    automatic_finish: false,
    proactive_prefilter: false,
    shell_risk: false,
    skill_suggestion: false,
    note_validation: false,
    reply_outcome: false,
  },
} as GroupRead

const routerOnly = ['Speaker selection', 'Automatic finish', 'Proactive pre-filter']
const everywhere = [
  'Shell command review',
  'Skill and MCP suggestion',
  'Group note validation',
  'Reply outcome',
]

describe('GroupDecisionSettingsSection', () => {
  afterEach(cleanup)

  it('offers every scenario outside router mode', () => {
    render(<GroupDecisionSettingsSection group={group} />)

    for (const name of [...routerOnly, ...everywhere]) {
      expect(screen.getByRole('switch', { name })).toBeInTheDocument()
    }
  })

  it('hides scenarios that never run in router mode', () => {
    render(<GroupDecisionSettingsSection group={{ ...group, scheduler_mode: 'router' }} />)

    for (const name of routerOnly) {
      expect(screen.queryByRole('switch', { name })).not.toBeInTheDocument()
    }
    for (const name of everywhere) {
      expect(screen.getByRole('switch', { name })).toBeInTheDocument()
    }
  })
})
