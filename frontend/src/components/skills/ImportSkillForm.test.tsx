import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { ImportSkillForm } from '@/components/skills/ImportSkillForm'
import i18n from '@/i18n'

vi.mock('@/hooks/useSkills', () => ({
  useImportSkill: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useImportSkillPackage: () => ({ mutateAsync: vi.fn(), isPending: false }),
  useImportSkillFromGithub: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))

describe('ImportSkillForm', () => {
  afterEach(async () => {
    cleanup()
    await i18n.changeLanguage('en-US')
  })

  it('retranslates a visible semantic error when the mounted locale changes', async () => {
    const user = userEvent.setup()
    render(<ImportSkillForm />)

    expect(screen.getByRole('tab', { name: 'Skill package (.zip)' })).toBeInTheDocument()
    await user.click(screen.getByRole('tab', { name: 'Paste SKILL.md' }))
    await user.click(screen.getByRole('button', { name: 'Import skill' }))
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Paste a SKILL.md before submitting.',
    )

    await i18n.changeLanguage('zh-CN')

    expect(await screen.findByRole('alert')).toHaveTextContent('提交前请先粘贴 SKILL.md。')
  })
})
