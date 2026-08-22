import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { UnsavedChangesProvider } from '@/components/layout/UnsavedChangesProvider'
import { SkillResourcesPanel } from '@/components/skills/SkillResourcesPanel'
import i18n from '@/i18n'
import type { SkillRead } from '@/types/api'

const mocks = vi.hoisted(() => ({
  update: vi.fn(),
  resources: [
    { path: 'references/first.md', size: 12, category: 'reference' },
    { path: 'scripts/second.ts', size: 24, category: 'script' },
  ],
  content: {
    'references/first.md': {
      path: 'references/first.md',
      size: 12,
      category: 'reference',
      is_text: true,
      content: 'first body',
    },
    'scripts/second.ts': {
      path: 'scripts/second.ts',
      size: 24,
      category: 'script',
      is_text: true,
      content: 'second body',
    },
  } as Record<string, {
    path: string
    size: number
    category: string
    is_text: boolean
    content: string
  }>,
}))

vi.mock('@/hooks/useSkills', () => ({
  useSkillResources: () => ({
    data: mocks.resources,
    error: null,
  }),
  useSkillResource: (_skillId: string, path: string | null) => ({
    data: path ? mocks.content[path] : undefined,
    isLoading: false,
    error: null,
  }),
  useUpdateSkillResource: () => ({ mutate: mocks.update, isPending: false }),
}))

const skill: SkillRead = {
  id: 'skill-1',
  name: 'Preview skill',
  description: null,
  body_markdown: '# Preview skill',
  metadata: null,
  source: 'import',
  files: [
    { path: 'references/first.md', size: 12, category: 'reference' },
    { path: 'scripts/second.ts', size: 24, category: 'script' },
  ],
  storage_path: null,
  status: 'active',
  created_at: '2026-08-22T00:00:00Z',
}

describe('SkillResourcesPanel', () => {
  beforeEach(async () => {
    mocks.update.mockReset()
    await i18n.changeLanguage('en-US')
  })

  afterEach(cleanup)

  it('guards a dirty resource before selecting another file', async () => {
    const user = userEvent.setup()
    render(
      <UnsavedChangesProvider>
        <SkillResourcesPanel skill={skill} />
      </UnsavedChangesProvider>,
    )

    const editor = await screen.findByRole('textbox', { name: 'editable text' })
    expect(editor).toHaveValue('first body')
    expect(screen.getByRole('button', { name: 'Save file' })).toBeDisabled()

    await user.clear(editor)
    await user.type(editor, 'local draft')
    expect(screen.getByRole('button', { name: 'Save file' })).toBeEnabled()

    await user.click(screen.getByRole('button', { name: /second\.ts/ }))
    expect(screen.getByRole('alertdialog')).toBeVisible()
    expect(editor).toHaveValue('local draft')

    await user.click(screen.getByRole('button', { name: 'Discard and leave' }))
    expect(await screen.findByDisplayValue('second body')).toBeVisible()
  })
})
