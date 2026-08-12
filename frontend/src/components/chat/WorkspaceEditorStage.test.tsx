import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceEditorStage } from '@/components/chat/WorkspaceEditorStage'
import i18n from '@/i18n'
import { useFileNavStore } from '@/stores/fileNavStore'
import type { ConversationWorkspaceFileRead } from '@/types/api'

vi.mock('@/components/chat/workspace-preview/WorkspacePreviewRouter', () => ({
  WorkspacePreviewRouter: ({
    file,
    onDirtyChange,
  }: {
    file: ConversationWorkspaceFileRead
    onDirtyChange?: (dirty: boolean) => void
  }) => (
    <div>
      preview:{file.path}
      <button type="button" onClick={() => onDirtyChange?.(true)}>edit {file.name}</button>
    </div>
  ),
}))

const firstFile: ConversationWorkspaceFileRead = {
  path: 'src/first.ts',
  name: 'first.ts',
  is_dir: false,
  size: 10,
  modified_at: null,
}

const secondFile: ConversationWorkspaceFileRead = {
  path: 'src/second.ts',
  name: 'second.ts',
  is_dir: false,
  size: 20,
  modified_at: null,
}

describe('WorkspaceEditorStage', () => {
  beforeEach(async () => {
    useFileNavStore.setState({ request: null, editorStages: {} })
    await i18n.changeLanguage('en-US')
  })

  afterEach(() => {
    cleanup()
    useFileNavStore.setState({ request: null, editorStages: {} })
  })

  it('keeps multiple editors mounted and confirms before closing a dirty tab', async () => {
    const user = userEvent.setup()
    useFileNavStore.getState().openEditor('group-1', firstFile)
    useFileNavStore.getState().openEditor('group-1', secondFile)

    render(
      <WorkspaceEditorStage scope="groups" conversationId="group-1">
        <div>message stage</div>
      </WorkspaceEditorStage>,
    )

    expect(screen.getByText('preview:src/first.ts')).toBeInTheDocument()
    expect(screen.getByText('preview:src/second.ts')).toBeVisible()
    await user.click(screen.getByRole('tab', { name: 'first.ts' }))
    expect(screen.getByText('preview:src/first.ts')).toBeVisible()
    expect(screen.getByText('preview:src/second.ts')).toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: 'edit first.ts' }))
    await user.click(screen.getByRole('button', { name: 'Close first.ts' }))
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Close first.ts?')

    await user.click(screen.getByRole('button', { name: 'Cancel' }))
    expect(screen.getByRole('tab', { name: 'first.ts' })).toBeVisible()
    await user.click(screen.getByRole('tab', { name: 'Chat' }))
    expect(screen.getByText('message stage')).toBeVisible()
  })
})
