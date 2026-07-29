import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceField } from '@/components/agents/WorkspaceField'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  workspaces: [
    {
      id: 'workspace-1',
      name: 'Project workspace',
      backend_type: 'local' as const,
      local_path: 'D:/projects/ag-swarmer',
      sandbox_ref: null,
      config: null,
      status: 'active',
      created_at: '2026-07-18T00:00:00Z',
      updated_at: '2026-07-18T00:00:00Z',
    },
  ],
  createWorkspace: { mutateAsync: vi.fn(), isPending: false },
}))

vi.mock('@/hooks/useWorkspaces', () => ({
  useWorkspaces: () => ({ data: mocks.workspaces }),
  useCreateWorkspace: () => mocks.createWorkspace,
}))

const defaultWorkspaces = mocks.workspaces

describe('WorkspaceField', () => {
  afterEach(async () => {
    cleanup()
    mocks.workspaces = defaultWorkspaces
    mocks.createWorkspace.mutateAsync.mockReset()
    await i18n.changeLanguage('en-US')
  })

  it('collapses only a long path, and keeps the full one in the title', () => {
    mocks.workspaces = [
      {
        ...mocks.workspaces[0],
        local_path: 'D:/very/deeply/nested/company/projects/2026/ag-swarmer/backend',
      },
    ]
    render(<WorkspaceField variant="compact" value="workspace-1" onChange={vi.fn()} />)

    const line = screen.getByText(/Location: …\/ag-swarmer\/backend/)
    expect(line).toBeInTheDocument()
    expect(line).toHaveAttribute(
      'title',
      'D:/very/deeply/nested/company/projects/2026/ag-swarmer/backend',
    )
  })

  it('uses a compact workspace selector without a duplicate label', () => {
    render(<WorkspaceField variant="compact" value="workspace-1" onChange={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'New workspace' })).toBeInTheDocument()
    expect(screen.queryByText('Workspace', { selector: 'label' })).not.toBeInTheDocument()
    expect(screen.getByText(/Location: D:\/projects\/ag-swarmer/)).toBeInTheDocument()
  })

  it('creates and selects a random workspace in one click', async () => {
    mocks.createWorkspace.mutateAsync.mockResolvedValueOnce({ id: 'workspace-2' })
    const onChange = vi.fn()
    render(
      <WorkspaceField
        value=""
        allowQuickCreate
        onChange={onChange}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Create workspace instantly' }))

    await waitFor(() => expect(onChange).toHaveBeenCalledWith('workspace-2'))
    expect(mocks.createWorkspace.mutateAsync).toHaveBeenCalledWith({
      name: expect.stringMatching(/^agent-[a-f0-9]{8}$/),
      backend_type: 'local',
      auto_create: true,
    })
  })

  it('translates the workspace label, picker action, and selected location', async () => {
    await i18n.changeLanguage('zh-CN')

    render(<WorkspaceField value="workspace-1" onChange={vi.fn()} />)

    expect(screen.getByLabelText('工作区')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '新建本地工作区' })).toBeInTheDocument()
    expect(screen.getByText(/绑定到本地：D:\/projects\/ag-swarmer/)).toBeInTheDocument()
  })
})
