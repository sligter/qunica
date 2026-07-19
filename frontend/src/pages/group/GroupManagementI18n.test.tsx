import { cleanup, render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import i18n from '@/i18n'
import type { GroupRead } from '@/types/api'

const mocks = vi.hoisted(() => ({
  group: null as GroupRead | null,
  mutateAsync: vi.fn(),
  navigate: vi.fn(),
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

vi.mock('@/hooks/useAgents', () => ({ useAgents: () => ({ data: [], isLoading: false }) }))
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
  useGroupAgents: () => ({ data: [], error: null, isLoading: false }),
}))
vi.mock('@/hooks/useGroupMessages', () => ({
  useGroupMessages: () => ({
    error: null,
    isLoading: false,
    hasNextPage: false,
    isFetchingNextPage: false,
    fetchNextPage: vi.fn(),
  }),
  useClearGroupMessages: () => ({ isPending: false, mutateAsync: mocks.mutateAsync }),
}))
vi.mock('@/hooks/useSendMessageStream', () => ({
  useSendMessageStream: () => ({
    error: null,
    isStreaming: false,
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
  useGroupMembers: () => ({ data: [], error: null, isLoading: false }),
  useMuteGroupMember: () => ({ isPending: false, mutate: vi.fn() }),
  useRemoveGroupMember: () => ({ isPending: false, mutate: vi.fn() }),
}))
vi.mock('@/hooks/useAddAgentToGroup', () => ({
  useAddAgentToGroup: () => ({ isPending: false, mutate: vi.fn() }),
}))
vi.mock('@/hooks/useGroupAgentActions', () => ({
  useMuteGroupAgent: () => ({ isPending: false, mutate: vi.fn() }),
  useRemoveGroupAgent: () => ({ isPending: false, mutate: vi.fn() }),
  useSetGroupAgentTopology: () => ({ isPending: false, mutate: vi.fn() }),
  useSetGroupAgentWorkspaceSharing: () => ({ isPending: false, mutate: vi.fn() }),
}))
vi.mock('@/hooks/useDeleteGroup', () => ({
  useDeleteGroup: () => ({ isPending: false, mutateAsync: mocks.mutateAsync }),
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
  proactive_max_rounds: 1,
  proactive_reply_multiplier: 2,
  allow_agent_free_mention: false,
  agent_free_mention_max_dispatches: 3,
  communication_mode: 'mesh',
  muted_agent_ids: null,
  admin_agent_ids: null,
  muted_member_ids: null,
  status: 'active',
  created_at: '2026-01-01T00:00:00Z',
  scheduler_enabled: false,
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

async function setLanguage(language: 'en-US' | 'zh-CN') {
  await i18n.changeLanguage(language)
}

describe('group management i18n', () => {
  beforeEach(() => {
    mocks.group = group
    mocks.mutateAsync.mockReset()
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
    expect(screen.getByRole('button', { name: '隐藏工作区文件' })).toBeVisible()
    expect(document.title).toBe('原样 Group 42 · AG Swarmer')
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
