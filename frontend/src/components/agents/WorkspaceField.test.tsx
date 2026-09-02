import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceField } from '@/components/agents/WorkspaceField'
import i18n from '@/i18n'

const mocks = vi.hoisted(() => ({
  workspaces: [
    {
      id: 'workspace-1',
      name: 'Project workspace',
      backend_type: 'local' as const,
      local_path: 'D:/projects/qunica',
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
        local_path: 'D:/very/deeply/nested/company/projects/2026/qunica/backend',
      },
    ]
    render(<WorkspaceField variant="compact" value="workspace-1" onChange={vi.fn()} />)

    const line = screen.getByText(/Location: …\/qunica\/backend/)
    expect(line).toBeInTheDocument()
    expect(line).toHaveAttribute(
      'title',
      'D:/very/deeply/nested/company/projects/2026/qunica/backend',
    )
  })

  it('uses a compact workspace selector without a duplicate label', () => {
    render(<WorkspaceField variant="compact" value="workspace-1" onChange={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'New workspace' })).toBeInTheDocument()
    expect(screen.queryByText('Workspace', { selector: 'label' })).not.toBeInTheDocument()
    expect(screen.getByText(/Location: D:\/projects\/qunica/)).toBeInTheDocument()
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

  it('creates a workspace from a directory name for remote backends', async () => {
    mocks.createWorkspace.mutateAsync.mockResolvedValueOnce({ id: 'workspace-2' })
    const onChange = vi.fn()
    render(<WorkspaceField value="" onChange={onChange} />)

    fireEvent.click(screen.getByRole('button', { name: 'New local workspace' }))
    fireEvent.change(screen.getByLabelText('Workspace name'), {
      target: { value: 'DSV4 Flash' },
    })
    fireEvent.change(screen.getByLabelText('Backend path or directory name'), {
      target: { value: 'dsv4-flash' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Create workspace' }))

    await waitFor(() => expect(onChange).toHaveBeenCalledWith('workspace-2'))
    expect(mocks.createWorkspace.mutateAsync).toHaveBeenCalledWith({
      name: 'DSV4 Flash',
      backend_type: 'local',
      local_path: 'dsv4-flash',
    })
  })

  it('selects additional workspaces without duplicating the primary', () => {
    mocks.workspaces = [
      mocks.workspaces[0],
      { ...mocks.workspaces[0], id: 'workspace-2', name: 'Reference workspace' },
    ]
    const onAdditionalChange = vi.fn()
    render(
      <WorkspaceField
        value="workspace-1"
        onChange={vi.fn()}
        additionalValues={[]}
        onAdditionalChange={onAdditionalChange}
      />,
    )

    expect(
      screen.queryByRole('checkbox', { name: /Project workspace/ }),
    ).not.toBeInTheDocument()
    fireEvent.click(
      screen.getByRole('checkbox', { name: /Reference workspace/ }),
    )
    expect(onAdditionalChange).toHaveBeenCalledWith(['workspace-2'])
  })

  it('keeps a large additional workspace library searchable and height-bounded', () => {
    const primary = mocks.workspaces[0]
    mocks.workspaces = [
      primary,
      ...Array.from({ length: 12 }, (_, index) => ({
        ...primary,
        id: `workspace-${index + 2}`,
        name: `Workspace ${index + 1}`,
        local_path: `D:/clients/client-${index + 1}`,
      })),
    ]

    render(
      <WorkspaceField
        value="workspace-1"
        onChange={vi.fn()}
        additionalValues={[]}
        onAdditionalChange={vi.fn()}
      />,
    )

    const list = screen.getByRole('listbox', { name: 'Additional workspaces' })
    expect(list).toHaveStyle({ maxHeight: '256px' })
    expect(within(list).queryByText('Project workspace')).not.toBeInTheDocument()
    expect(screen.getByText('12 workspaces · 0 mounted')).toBeInTheDocument()

    fireEvent.change(screen.getByLabelText('Search workspaces'), {
      target: { value: 'client-11' },
    })

    expect(within(list).getByText('Workspace 11').closest('[data-picker-row]')).toHaveTextContent(
      'local · D:/clients/client-11',
    )
    expect(within(list).queryByText('Workspace 2')).not.toBeInTheDocument()
  })

  it('translates the workspace label, picker action, and selected location', async () => {
    await i18n.changeLanguage('zh-CN')

    render(<WorkspaceField value="workspace-1" onChange={vi.fn()} />)

    expect(screen.getByLabelText('工作区')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '新建本地工作区' })).toBeInTheDocument()
    expect(screen.getByText(/绑定到本地：D:\/projects\/qunica/)).toBeInTheDocument()
  })
})
