import { cleanup, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import { ApiError } from '@/lib/api-v2/client'
import type { GroupAgentRead, GroupMemberRead, GroupRead, GroupThread } from '@/types/api'

const mocks = vi.hoisted(() => ({
  group: null as GroupRead | null,
  groupAgents: [] as GroupAgentRead[],
  groupMembers: [] as GroupMemberRead[],
  agents: [] as { id: string; workspace_id: string | null }[],
  workspaces: [] as { id: string; local_path: string | null }[],
  groupThreads: [] as GroupThread[],
  setWorkspaceMode: vi.fn(),
  mutateAsync: vi.fn(),
  createTaskMutateAsync: vi.fn(),
  createTaskReset: vi.fn(),
  archiveTaskMutateAsync: vi.fn(),
  isStreaming: false,
  clearMutateAsync: vi.fn(),
  deleteMutateAsync: vi.fn(),
  closeConversation: vi.fn(),
  toggleDock: vi.fn(),
  registerTerminal: vi.fn(),
}))

vi.mock('@/components/agents/WorkspaceField', () => ({
  WorkspaceField: ({ value }: { value: string }) => (
    <input aria-label="Workspace picker" readOnly value={value} />
  ),
}))

vi.mock('@/components/chat/Composer', () => ({
  Composer: ({ hint }: { hint?: string }) => <div>{hint}</div>,
}))
vi.mock('@/components/chat/GroupNotesPanel', () => ({
  GroupNotesPanel: () => <div>raw note body</div>,
}))
vi.mock('@/components/chat/GroupWorkspacePanel', () => ({
  GroupWorkspacePanel: () => <div>workspace panel</div>,
}))
vi.mock('@/components/chat/MessageList', () => ({ MessageList: () => <div>message list</div> }))
vi.mock('@/components/chat/TurnTraceDrawer', () => ({ TurnTraceDrawer: () => null }))
vi.mock('@/components/layout/VerticalResizeHandle', () => ({
  VerticalResizeHandle: ({ label }: { label: string }) => <div aria-label={label} />,
}))
vi.mock('@/pages/group/GroupSchedulerSettingsSection', () => ({
  GroupSchedulerSettingsSection: () => <div>scheduler section</div>,
}))

vi.mock('@/hooks/useAgents', () => ({
  useAgents: () => ({ data: mocks.agents, isLoading: false }),
}))
vi.mock('@/hooks/useCreateGroup', () => ({
  useCreateGroup: () => ({ isPending: false, mutateAsync: mocks.mutateAsync }),
}))
vi.mock('@/hooks/useSystemSettings', () => ({
  useSystemSettings: () => ({
    data: { group_workspace_root: 'D:/groups' },
    isLoading: false,
  }),
}))
vi.mock('@/hooks/useGroups', () => ({
  useGroup: () => ({ data: mocks.group, error: null, isLoading: false }),
  useUpdateGroup: () => ({ isPending: false, mutateAsync: mocks.mutateAsync }),
}))
vi.mock('@/hooks/useGroupAgents', () => ({
  useGroupAgents: () => ({ data: mocks.groupAgents, error: null, isLoading: false }),
}))
vi.mock('@/hooks/useGroupMessages', () => ({
  useConversationMessages: () => ({
    error: null,
    isLoading: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
  }),
  useGroupMessages: () => ({
    error: null,
    isLoading: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
  }),
  useClearGroupMessages: () => ({ isPending: false, mutateAsync: mocks.clearMutateAsync }),
}))
vi.mock('@/hooks/useGroupThreads', () => ({
  useGroupThreads: () => ({ data: mocks.groupThreads, error: null, isLoading: false }),
  useCreateGroupThread: () => ({
    error: null,
    isPending: false,
    mutateAsync: mocks.createTaskMutateAsync,
    reset: mocks.createTaskReset,
  }),
  useArchiveGroupThread: () => ({
    isPending: false,
    mutateAsync: mocks.archiveTaskMutateAsync,
  }),
}))
vi.mock('@/hooks/useSendMessageStream', () => ({
  useSendMessageStream: () => ({
    error: null,
    isStreaming: mocks.isStreaming,
    send: vi.fn(),
    cancel: vi.fn(),
  }),
}))
vi.mock('@/hooks/usePersistentPaneWidth', () => ({
  usePersistentPaneWidth: () => ({
    width: 280,
    minWidth: 240,
    maxWidth: 560,
    startResize: vi.fn(),
    resizeBy: vi.fn(),
  }),
}))
vi.mock('@/hooks/useGroupMembers', () => ({
  useAddGroupMember: () => ({ isPending: false, mutate: vi.fn() }),
  useGroupMemberCandidates: () => ({ data: [] }),
  useGroupMembers: () => ({ data: mocks.groupMembers, error: null, isLoading: false }),
  useMuteGroupMember: () => ({ isPending: false, mutate: vi.fn() }),
  useRemoveGroupMember: () => ({ isPending: false, mutate: vi.fn() }),
}))
vi.mock('@/hooks/useAddAgentToGroup', () => ({
  useAddAgentToGroup: () => ({ isPending: false, mutate: vi.fn() }),
}))
vi.mock('@/hooks/useWorkspaces', () => ({
  useWorkspaces: () => ({ data: mocks.workspaces, isLoading: false }),
}))
vi.mock('@/hooks/useGroupAgentActions', () => ({
  useMuteGroupAgent: () => ({ isPending: false, mutate: vi.fn() }),
  useRemoveGroupAgent: () => ({ isPending: false, mutate: vi.fn() }),
  useSetGroupAgentTopology: () => ({ isPending: false, mutate: vi.fn() }),
  useSetGroupAgentWorkspaceMode: () => ({ isPending: false, mutate: mocks.setWorkspaceMode }),
}))
vi.mock('@/hooks/useDeleteGroup', () => ({
  useDeleteGroup: () => ({ isPending: false, mutateAsync: mocks.deleteMutateAsync }),
}))
vi.mock('@/terminal/TerminalRuntimeProvider', () => ({
  useTerminalRuntime: () => ({
    closeConversation: mocks.closeConversation,
    isDockOpen: false,
    toggleDock: mocks.toggleDock,
  }),
}))
vi.mock('@/terminal/useTerminalConversationRegistration', () => ({
  useTerminalConversationRegistration: mocks.registerTerminal,
}))

import { GroupFormDialog } from '@/components/groups/GroupFormDialog'
import { GroupChatPage } from '@/pages/group/GroupChatPage'
import { GroupManagePage } from '@/pages/group/GroupManagePage'
import { GroupMembersTab } from '@/pages/group/GroupMembersTab'
import { GroupSettingsTab } from '@/pages/group/GroupSettingsTab'

const group: GroupRead = {
  id: 'group-1',
  workspace_id: null,
  name: '原样 Group 42',
  description: null,
  announcement: 'RAW announcement / 路径 C:/work',
  free_speech: false,
  proactive_mode: false,
  allow_agent_free_mention: false,
  agent_free_mention_max_dispatches: 3,
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

const humanMember: GroupMemberRead = {
  id: 'member-1',
  group_id: 'group-1',
  user_id: 'user-1',
  display_name: 'Human One',
  role: 'member',
  status: 'active',
  is_muted: false,
  joined_at: '2026-01-01T00:00:00Z',
}

const groupAgent: GroupAgentRead = {
  id: 'group-agent-1',
  group_id: 'group-1',
  agent_id: 'agent-1',
  display_name: 'Agent One',
  role: 'agent',
  topology_role: null,
  speaking_order: null,
  response_mode: 'default',
  workspace_mode: 'self',
  share_group_workspace: false,
  context_usage: null,
  status: 'active',
  joined_at: '2026-01-01T00:00:00Z',
}

const taskThread: GroupThread = {
  id: 'thread-1',
  group_id: 'group-1',
  agent_id: null,
  created_by: null,
  thread_type: 'task_thread',
  title: 'Existing task',
  goal: null,
  status: 'active',
  priority: 0,
  started_at: null,
  completed_at: null,
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z',
}

async function setLanguage(language: 'en-US' | 'zh-CN') {
  await i18n.changeLanguage(language)
}

describe('group management i18n', () => {
  beforeEach(() => {
    mocks.group = group
    mocks.groupAgents = []
    mocks.groupMembers = []
    mocks.agents = []
    mocks.workspaces = []
    mocks.groupThreads = [taskThread]
    mocks.setWorkspaceMode.mockReset()
    mocks.mutateAsync.mockReset()
    mocks.createTaskMutateAsync.mockReset().mockImplementation(async (title: string) => {
      const created = { ...taskThread, id: 'thread-2', title }
      mocks.groupThreads = [created, ...mocks.groupThreads]
      return created
    })
    mocks.createTaskReset.mockReset()
    mocks.archiveTaskMutateAsync.mockReset()
    mocks.isStreaming = false
    mocks.clearMutateAsync.mockReset()
    mocks.deleteMutateAsync.mockReset()
    mocks.closeConversation.mockReset().mockResolvedValue(undefined)
    mocks.toggleDock.mockReset().mockResolvedValue(undefined)
    mocks.registerTerminal.mockReset()
    window.localStorage.removeItem('ag-swarmer:groups:selected-thread:group-1')
  })

  afterEach(async () => {
    cleanup()
    document.title = ''
    await setLanguage('en-US')
  })

  it('localizes the group creation dialog without translating workspace data', async () => {
    await setLanguage('zh-CN')
    render(
      <MemoryRouter>
        <GroupFormDialog open onOpenChange={vi.fn()} />
      </MemoryRouter>,
    )

    expect(screen.getByRole('heading', { name: '创建新群组' })).toBeVisible()
    expect(screen.getByLabelText('群组名称')).toBeVisible()
    expect(screen.getByText('D:/groups')).toBeVisible()
    expect(screen.getByRole('button', { name: '关闭' })).toBeVisible()
  })

  it('shows localized framing for an unexpected group creation error', async () => {
    const user = userEvent.setup()
    mocks.mutateAsync.mockRejectedValueOnce(new Error('socket closed'))
    await setLanguage('zh-CN')
    render(
      <MemoryRouter>
        <GroupFormDialog open onOpenChange={vi.fn()} />
      </MemoryRouter>,
    )

    await user.type(screen.getByLabelText('群组名称'), '测试群组')
    await user.click(screen.getByRole('button', { name: '创建群组' }))

    expect(await screen.findByRole('alert')).toHaveTextContent('创建群组失败')
  })

  it('localizes group chat framing while preserving group data raw', async () => {
    mocks.group = { ...group, workspace_id: 'workspace-1' }
    await setLanguage('zh-CN')
    render(
      <MemoryRouter initialEntries={['/groups/group-1']}>
        <Routes>
          <Route path="/groups/:groupId" element={<GroupChatPage />} />
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByRole('heading', { name: '原样 Group 42' })).toBeVisible()
    expect(screen.getByText('公告：RAW announcement / 路径 C:/work')).toBeVisible()
    expect(screen.getByRole('button', { name: '新任务' })).toBeVisible()
    expect(screen.getByRole('button', { name: '隐藏工作区文件' })).toBeVisible()
    expect(document.title).toBe('原样 Group 42 · AG Swarmer')
    expect(mocks.registerTerminal).toHaveBeenCalledWith('group-1', 'workspace-1')
  })

  it('creates and selects a named group task', async () => {
    const user = userEvent.setup()
    await setLanguage('en-US')
    render(
      <MemoryRouter initialEntries={['/groups/group-1']}>
        <Routes>
          <Route path="/groups/:groupId" element={<GroupChatPage />} />
        </Routes>
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: 'Start new task' }))
    expect(mocks.createTaskMutateAsync).not.toHaveBeenCalled()
    const dialog = screen.getByRole('dialog')
    expect(within(dialog).getByRole('heading', { name: 'Start a new task' })).toBeVisible()
    await user.type(within(dialog).getByLabelText('Task title'), 'Release checklist')
    await user.click(within(dialog).getByRole('button', { name: 'Start new task' }))
    await waitFor(() => {
      expect(mocks.createTaskMutateAsync).toHaveBeenCalledWith('Release checklist')
      expect(window.localStorage.getItem('ag-swarmer:groups:selected-thread:group-1'))
        .toBe('thread-2')
    })
  })

  it('restores the last selected task when the group is reopened', async () => {
    mocks.groupThreads = [
      taskThread,
      { ...taskThread, id: 'thread-2', title: 'Second task' },
    ]
    window.localStorage.setItem('ag-swarmer:groups:selected-thread:group-1', 'thread-2')

    render(
      <MemoryRouter initialEntries={['/groups/group-1']}>
        <Routes>
          <Route path="/groups/:groupId" element={<GroupChatPage />} />
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByRole('combobox', { name: 'Current task' })).toHaveTextContent('Second task')
  })

  it('keeps task switching and creation available while the current task runs', () => {
    mocks.isStreaming = true
    mocks.groupThreads = [
      taskThread,
      { ...taskThread, id: 'thread-2', title: 'Second task' },
    ]

    render(
      <MemoryRouter initialEntries={['/groups/group-1']}>
        <Routes>
          <Route path="/groups/:groupId" element={<GroupChatPage />} />
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByRole('combobox', { name: 'Current task' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Start new task' })).toBeEnabled()
    expect(screen.getByRole('button', { name: 'Archive task' })).toBeDisabled()
  })

  it('localizes the manage shell and tabs while preserving the group name', async () => {
    await setLanguage('zh-CN')
    render(
      <MemoryRouter initialEntries={['/groups/group-1/manage?tab=notes']}>
        <Routes>
          <Route path="/groups/:groupId/manage" element={<GroupManagePage />} />
        </Routes>
      </MemoryRouter>,
    )

    expect(screen.getByText('管理群组')).toBeVisible()
    expect(screen.getByText('原样 Group 42')).toBeVisible()
    expect(screen.getByRole('tab', { name: '成员' })).toBeVisible()
    expect(screen.getByRole('tab', { name: '笔记' })).toBeVisible()
    expect(document.title).toBe('原样 Group 42 · 管理 · AG Swarmer')

    await setLanguage('en-US')
    expect(document.title).toBe('原样 Group 42 · Manage · AG Swarmer')
  })

  it('localizes member empty and action framing', async () => {
    await setLanguage('zh-CN')
    render(
      <MemoryRouter>
        <GroupMembersTab groupId="group-1" />
      </MemoryRouter>,
    )

    expect(screen.getByRole('heading', { name: '成员' })).toBeVisible()
    expect(screen.getByPlaceholderText('搜索成员')).toBeVisible()
    expect(screen.getByText('没有匹配的成员。')).toBeVisible()
    expect(screen.getByRole('heading', { name: '添加 Agent' })).toBeVisible()
  })

  it('pluralizes the raw member count while displaying the formatted count', async () => {
    await setLanguage('en-US')
    mocks.groupMembers = [humanMember]
    render(
      <MemoryRouter>
        <GroupMembersTab groupId="group-1" />
      </MemoryRouter>,
    )

    expect(screen.getByText('1 person or agent in this group.')).toBeVisible()

    cleanup()
    mocks.groupAgents = [groupAgent]
    render(
      <MemoryRouter>
        <GroupMembersTab groupId="group-1" />
      </MemoryRouter>,
    )

    expect(screen.getByText('2 people and agents in this group.')).toBeVisible()
  })

  it('offers the three workspace modes and shows where each one resolves', async () => {
    const user = userEvent.setup()
    mocks.group = { ...group, workspace_id: 'ws-group' }
    mocks.groupAgents = [{ ...groupAgent, workspace_mode: 'group_and_self' }]
    mocks.agents = [{ id: 'agent-1', workspace_id: 'ws-own' }]
    mocks.workspaces = [
      { id: 'ws-group', local_path: 'D:/groups/alpha' },
      { id: 'ws-own', local_path: 'D:/agents/one' },
    ]
    render(
      <MemoryRouter>
        <GroupMembersTab groupId="group-1" />
      </MemoryRouter>,
    )

    // A non-default mode is tagged on the row; the default would not be.
    expect(screen.getByText('Group + its own folder')).toBeVisible()
    await user.click(screen.getByText('Agent One').closest('button')!)

    const select = screen.getByRole('combobox', { name: 'Workspace access' })
    expect(select).toHaveValue('group_and_self')
    for (const label of ['Group workspace', 'Group + its own folder', 'Its own folder only']) {
      expect(within(select).getByRole('option', { name: label })).toBeInTheDocument()
    }
    // Both resolved roots are visible, so the consequence of the mode is not hidden.
    expect(screen.getByText('Plain paths resolve in: D:/groups/alpha')).toBeVisible()
    expect(screen.getByText('Mounted at ~self/: D:/agents/one')).toBeVisible()

    await user.selectOptions(select, 'self')
    expect(mocks.setWorkspaceMode).toHaveBeenCalledWith(
      { groupId: 'group-1', agentId: 'agent-1', workspaceMode: 'self' },
      expect.anything(),
    )
  })

  it('resolves plain paths to the agent folder when the agent is isolated', async () => {
    const user = userEvent.setup()
    mocks.group = { ...group, workspace_id: 'ws-group' }
    mocks.groupAgents = [groupAgent]
    mocks.agents = [{ id: 'agent-1', workspace_id: 'ws-own' }]
    mocks.workspaces = [
      { id: 'ws-group', local_path: 'D:/groups/alpha' },
      { id: 'ws-own', local_path: 'D:/agents/one' },
    ]
    render(
      <MemoryRouter>
        <GroupMembersTab groupId="group-1" />
      </MemoryRouter>,
    )

    await user.click(screen.getByText('Agent One').closest('button')!)
    expect(screen.getByText('Plain paths resolve in: D:/agents/one')).toBeVisible()
    expect(screen.queryByText(/Mounted at/)).not.toBeInTheDocument()
    expect(
      screen.getByText(
        'Isolated: group files and message attachments are out of reach, and its output stays out of the group workspace.',
      ),
    ).toBeVisible()
  })

  it('does not mislabel a null hierarchical topology role as Worker', async () => {
    const user = userEvent.setup()
    mocks.group = { ...group, communication_mode: 'hierarchical' }
    mocks.groupAgents = [groupAgent]
    render(
      <MemoryRouter>
        <GroupMembersTab groupId="group-1" />
      </MemoryRouter>,
    )

    expect(screen.queryByText('Worker')).not.toBeInTheDocument()
    await user.click(screen.getByText('Agent One').closest('button')!)
    expect(screen.getByRole('combobox', { name: 'Hierarchy role' })).toHaveValue(
      '__none__',
    )
    expect(screen.getByRole('option', { name: 'No topology role' })).toBeVisible()
  })

  it('preserves an unknown hierarchical topology role in the row and select', async () => {
    const user = userEvent.setup()
    mocks.group = { ...group, communication_mode: 'hierarchical' }
    mocks.groupAgents = [
      { ...groupAgent, topology_role: 'future-role' } as unknown as GroupAgentRead,
    ]
    render(
      <MemoryRouter>
        <GroupMembersTab groupId="group-1" />
      </MemoryRouter>,
    )

    expect(screen.getByText('Unknown topology role: future-role')).toBeVisible()
    await user.click(screen.getByText('Agent One').closest('button')!)

    expect(screen.getByRole('combobox', { name: 'Hierarchy role' })).toHaveValue(
      'future-role',
    )
    expect(
      screen.getByRole('option', { name: 'Unknown topology role: future-role' }),
    ).toBeVisible()
  })

  it('retranslates settings around an existing unsaved draft', async () => {
    const user = userEvent.setup()
    await setLanguage('en-US')
    const { rerender } = render(
      <MemoryRouter>
        <GroupSettingsTab group={group} />
      </MemoryRouter>,
    )
    const name = screen.getByLabelText('Group name')
    await user.clear(name)
    await user.type(name, 'draft')

    await setLanguage('zh-CN')
    rerender(
      <MemoryRouter>
        <GroupSettingsTab group={group} />
      </MemoryRouter>,
    )

    expect(screen.getByText('基本信息')).toBeVisible()
    expect(screen.getByLabelText('群组名称')).toHaveValue('draft')
    expect(screen.getByRole('switch', { name: '自由发言' })).toBeVisible()
  })

  it('shows localized framing for an unexpected settings update error', async () => {
    const user = userEvent.setup()
    mocks.mutateAsync.mockRejectedValueOnce(new Error('offline'))
    await setLanguage('zh-CN')
    render(
      <MemoryRouter>
        <GroupSettingsTab group={group} />
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('switch', { name: '自由发言' }))
    expect(await screen.findByRole('alert')).toHaveTextContent('更新群组失败')
  })

  it('retranslates a clear-history ApiError while preserving its raw diagnostic', async () => {
    const user = userEvent.setup()
    mocks.clearMutateAsync.mockRejectedValueOnce(
      new ApiError(500, 'server_error', 'RAW clear diagnostic'),
    )
    await setLanguage('en-US')
    render(
      <MemoryRouter>
        <GroupSettingsTab group={group} />
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: 'Clear history' }))
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Clear' }),
    )
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to clear chat history: RAW clear diagnostic',
    )

    await setLanguage('zh-CN')
    expect(screen.getByRole('alert')).toHaveTextContent(
      '清除聊天记录失败：RAW clear diagnostic',
    )
    expect(screen.queryByText(/Failed to clear chat history/)).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '清除记录' }))
    expect(
      within(screen.getByRole('alertdialog')).queryByRole('alert'),
    ).not.toBeInTheDocument()
    expect(
      within(screen.getByRole('alertdialog')).queryByText(/Failed to clear chat history/),
    ).not.toBeInTheDocument()
  })

  it('retranslates a delete-group non-Error while preserving its raw diagnostic', async () => {
    const user = userEvent.setup()
    mocks.deleteMutateAsync.mockRejectedValueOnce('RAW delete diagnostic')
    await setLanguage('en-US')
    render(
      <MemoryRouter>
        <GroupSettingsTab group={group} />
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: 'Delete group' }))
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }),
    )
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Failed to delete group: RAW delete diagnostic',
    )

    await setLanguage('zh-CN')
    expect(screen.getByRole('alert')).toHaveTextContent(
      '删除群组失败：RAW delete diagnostic',
    )
    expect(screen.queryByText(/Failed to delete group/)).not.toBeInTheDocument()

    await user.click(screen.getByRole('button', { name: '删除群组' }))
    expect(
      within(screen.getByRole('alertdialog')).queryByRole('alert'),
    ).not.toBeInTheDocument()
    expect(
      within(screen.getByRole('alertdialog')).queryByText(/Failed to delete group/),
    ).not.toBeInTheDocument()
    expect(mocks.closeConversation).not.toHaveBeenCalled()
  })

  it('closes group terminals only after backend deletion succeeds', async () => {
    const user = userEvent.setup()
    mocks.deleteMutateAsync.mockResolvedValueOnce(undefined)
    mocks.closeConversation.mockResolvedValueOnce(undefined)
    render(
      <MemoryRouter>
        <GroupSettingsTab group={group} />
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: 'Delete group' }))
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }),
    )

    await waitFor(() => expect(mocks.closeConversation).toHaveBeenCalledWith('group-1', true))
    expect(mocks.deleteMutateAsync.mock.invocationCallOrder[0]).toBeLessThan(
      mocks.closeConversation.mock.invocationCallOrder[0]!,
    )
  })

  it('continues group deletion after terminal cleanup fails', async () => {
    const user = userEvent.setup()
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    mocks.deleteMutateAsync.mockResolvedValueOnce(undefined)
    mocks.closeConversation.mockRejectedValueOnce({
      code: 'terminal.cleanup_timeout', message: 'Cleanup timed out',
    })
    render(
      <MemoryRouter>
        <GroupSettingsTab group={group} />
      </MemoryRouter>,
    )

    await user.click(screen.getByRole('button', { name: 'Delete group' }))
    await user.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }),
    )

    await waitFor(() => expect(screen.queryByRole('alertdialog')).not.toBeInTheDocument())
    expect(consoleError).toHaveBeenCalledWith('[terminal] cleanup failed', {
      code: 'terminal.cleanup_timeout', message: 'Cleanup timed out',
    })
    consoleError.mockRestore()
  })

  it('preserves an unknown communication mode from the wire', async () => {
    mocks.group = { ...group, communication_mode: 'future-mode' } as unknown as GroupRead
    await setLanguage('en-US')
    render(
      <MemoryRouter>
        <GroupSettingsTab group={mocks.group} />
      </MemoryRouter>,
    )

    expect(screen.getByRole('combobox', { name: 'Communication mode' })).toHaveValue(
      'future-mode',
    )
    expect(screen.getByRole('option', { name: 'future-mode' })).toBeVisible()
  })
})
