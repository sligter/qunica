import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { WorkspaceField } from '@/components/agents/WorkspaceField'

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
}))

vi.mock('@/hooks/useWorkspaces', () => ({
  useWorkspaces: () => ({ data: mocks.workspaces }),
  useCreateWorkspace: () => ({ mutateAsync: vi.fn(), isPending: false }),
}))

describe('WorkspaceField', () => {
  afterEach(cleanup)

  it('uses a compact workspace selector without a duplicate label', () => {
    render(<WorkspaceField variant="compact" value="workspace-1" onChange={vi.fn()} />)

    expect(screen.getByRole('button', { name: 'New workspace' })).toBeInTheDocument()
    expect(screen.queryByText('Workspace', { selector: 'label' })).not.toBeInTheDocument()
    expect(screen.getByText(/Location: D:\/projects\/ag-swarmer/)).toBeInTheDocument()
  })
})
