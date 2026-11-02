import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { StreamTimeline } from '@/components/chat/StreamTimeline'
import i18n from '@/i18n'
import { useMessageStore } from '@/stores/messageStore'
import type { StreamRun, StreamTimelineEvent, StreamToolEvent } from '@/stores/messageStore'

vi.mock('@/hooks/useGroupAgents', () => ({
  useGroupAgents: () => ({ data: [] }),
}))

vi.mock('@/components/chat/MarkdownMessage', () => ({
  MarkdownMessage: ({ content }: { content: string }) => <div>{content}</div>,
}))

vi.mock('@/components/assistant/AssistantApprovalCard', () => ({
  AssistantApprovalCard: ({ action }: { action: { summary: string } }) => (
    <div data-testid="assistant-approval">{action.summary}</div>
  ),
}))

function event<T extends StreamTimelineEvent>(value: T): T {
  return value
}

function run(events: StreamTimelineEvent[], status: StreamRun['status'] = 'completed'): StreamRun {
  return {
    id: 'stream-1',
    group_id: 'group-1',
    user_message_id: 'message-1',
    status,
    turn_id: null,
    scheduler_status: null,
    moderator_active: false,
    terminal_reason: null,
    criticalSummaries: [],
    created_at: '2026-07-16T10:00:00Z',
    updated_at: '2026-07-16T10:00:06Z',
    events,
  }
}

afterEach(async () => {
  cleanup()
  useMessageStore.setState(useMessageStore.getInitialState(), true)
  await i18n.changeLanguage('en-US')
})

describe('StreamTimeline per-agent completion', () => {
  const identity = {
    stream_id: 'stream-1', agent_id: 'agent-1', display_name: 'Researcher',
    created_at: '2026-07-16T10:00:04Z',
  }
  const pendingTool: StreamToolEvent = {
    ...identity, id: 'pending-tool', type: 'tool', tool_call_id: 'call-1',
    tool_name: 'take_screenshot', status: 'started',
  }
  const reply: StreamTimelineEvent = {
    ...identity, id: 'reply', type: 'agent_message', message_id: 'message-2', content: 'Finished reply',
  }
  const completions: Array<{ name: string; event: StreamTimelineEvent }> = [
    { name: 'persisted reply', event: reply },
    { name: 'finalized reply', event: {
      ...identity, id: 'final-draft', type: 'response_draft', status: 'finalized',
      message_id: 'message-2', content: 'Finished reply',
    } },
    { name: 'silence', event: { ...identity, id: 'silent', type: 'agent_silent', message: 'No visible reply' } },
    { name: 'error', event: { ...identity, id: 'error', type: 'agent_error', message: 'Agent failed' } },
    { name: 'handoff', event: { ...identity, id: 'handoff', type: 'agent_handoff', message: 'Handoff accepted' } },
  ]

  it.each(completions)('settles $name while another agent keeps running', ({ event: completion }) => {
    render(<StreamTimeline run={run([
      pendingTool, completion,
      { ...pendingTool, id: 'other-tool', tool_call_id: 'call-2', agent_id: 'agent-2',
        display_name: 'Writer', tool_name: 'Write' },
    ], 'active')} />)
    expect(screen.queryByText('Running take_screenshot')).not.toBeInTheDocument()
    expect(screen.getByText('Running Write')).toBeVisible()
    expect(screen.getAllByRole('status')).toHaveLength(1)
    const finishedActivity = screen.getByRole('group', { name: 'Activity: 1 tool' })
    expect(within(finishedActivity).getByText('No result received')).toBeInTheDocument()
    expect(screen.getByRole('group', { name: 'Activity: 1 tool, active' })).toBeInTheDocument()
    expect(pendingTool.status).toBe('started')
  })

  it('shows moderator scheduling after a reply with a missing tool result', () => {
    render(<StreamTimeline moderatorEnabled run={{
      ...run([pendingTool, reply], 'active'), moderator_active: true,
    }} />)
    expect(screen.getByText('Choosing the next speaker…')).toBeVisible()
    expect(screen.queryByText('Running take_screenshot')).not.toBeInTheDocument()
  })

  it('settles earlier activity when another agent interleaves before completion', () => {
    render(<StreamTimeline run={run([
      pendingTool,
      { ...pendingTool, id: 'other-tool', tool_call_id: 'call-2', agent_id: 'agent-2',
        display_name: 'Writer', tool_name: 'Write' },
      reply,
    ], 'active')} />)
    expect(screen.queryByText('Running take_screenshot')).not.toBeInTheDocument()
    expect(screen.getByText('Running Write')).toBeVisible()
    expect(screen.getAllByRole('status')).toHaveLength(1)
    expect(screen.getByRole('group', { name: 'Activity: 1 tool' })).toBeInTheDocument()
  })

  it('keeps the next execution by the same agent separate from unfinished old tools', () => {
    render(<StreamTimeline run={run([
      pendingTool, reply,
      { ...identity, id: 'next-start', type: 'agent_start' },
      { ...pendingTool, id: 'next-tool', tool_call_id: 'call-2', tool_name: 'Read' },
    ], 'active')} />)
    expect(screen.getByText('Running Read')).toBeVisible()
    expect(screen.getAllByText('Researcher')).toHaveLength(2)
    expect(within(screen.getByRole('group', { name: 'Activity: 1 tool' }))
      .getByText('No result received')).toBeInTheDocument()
    expect(within(screen.getByRole('group', { name: 'Activity: 1 tool, active' }))
      .getByText('started')).toBeInTheDocument()
  })

  it('keeps intermediate finalized text in the same active execution', () => {
    render(<StreamTimeline run={run([
      { ...identity, id: 'intermediate', type: 'response_draft', status: 'finalized', content: 'Checking…' },
      pendingTool,
    ], 'active')} />)
    expect(screen.getByText('Running take_screenshot')).toBeVisible()
    expect(screen.getAllByText('Researcher')).toHaveLength(1)
    expect(screen.queryByText('No result received')).not.toBeInTheDocument()
  })

  it('settles tools after the last text segment when the actual final message arrives', () => {
    useMessageStore.setState(useMessageStore.getInitialState(), true)
    const store = useMessageStore.getState()
    // The provider can finish without another token after a tool. The store
    // finalizes text in place, before the last tool in the timeline.
    store.patchStreamDraft('group-1', 'stream-1', 'agent-1', 'Finished reply', 'Researcher')
    store.upsertStreamTool('group-1', 'stream-1', {
      id: 'call-1', agent_id: 'agent-1', display_name: 'Researcher',
      tool_name: 'take_screenshot', status: 'started',
    })
    store.finalizeStreamDraft('group-1', 'stream-1', {
      id: 'message-2', group_id: 'group-1', thread_id: 'thread-1', sender_id: 'agent-1',
      sender_type: 'agent', message_type: 'text', content: 'Finished reply', status: 'visible',
      attachments: [], refs: null, context_usage: null, turn_id: null, dispatch_id: null,
      reply_to_message_id: null, turn_summary: null, created_at: identity.created_at,
    }, 'Researcher')
    const snapshot = useMessageStore.getState().streamRunsByGroup['group-1']['stream-1']
    const { rerender } = render(<StreamTimeline run={{ ...snapshot, moderator_active: true }} moderatorEnabled />)
    expect(screen.queryByText('Running take_screenshot')).not.toBeInTheDocument()
    expect(screen.getByText('No result received')).toBeInTheDocument()
    expect(screen.getByText('Choosing the next speaker…')).toBeVisible()
    expect(screen.getAllByText('Researcher')).toHaveLength(1)
    expect(screen.getAllByText('Finished reply')).toHaveLength(1)
    store.upsertStreamTool('group-1', 'stream-1', {
      id: 'call-1', agent_id: 'agent-1', display_name: 'Researcher',
      tool_name: 'take_screenshot', status: 'failed', result_summary: 'Timed out',
    })
    rerender(<StreamTimeline moderatorEnabled run={{
      ...useMessageStore.getState().streamRunsByGroup['group-1']['stream-1'], moderator_active: true,
    }} />)
    expect(screen.getByText('failed')).toBeInTheDocument()
    expect(screen.queryByText('No result received')).not.toBeInTheDocument()
    expect(screen.queryByText('Running take_screenshot')).not.toBeInTheDocument()
    expect(screen.getByText('Choosing the next speaker…')).toBeVisible()
  })
})

describe('StreamTimeline activity rendering', () => {
  it('names the phase an agent is in rather than only that it is busy', async () => {
    await i18n.changeLanguage('en-US')
    const agentStart = event({
      id: 'agent-start:stream-1:agent-1',
      stream_id: 'stream-1',
      type: 'agent_start' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      index: 0,
      total: 1,
      created_at: '2026-07-16T10:00:01Z',
    })
    const reasoning = event({
      id: 'reasoning-1',
      stream_id: 'stream-1',
      type: 'reasoning' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      content: 'Working through it.',
      status: 'streaming' as const,
      created_at: '2026-07-16T10:00:02Z',
    })
    const tool = event({
      id: 'tool-1',
      stream_id: 'stream-1',
      type: 'tool' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      tool_call_id: 'call-1',
      tool_name: 'Read',
      status: 'started' as const,
      created_at: '2026-07-16T10:00:03Z',
    })
    const draft = event({
      id: 'draft-1',
      stream_id: 'stream-1',
      type: 'response_draft' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      content: 'Here is what I found',
      status: 'streaming' as const,
      created_at: '2026-07-16T10:00:04Z',
    })

    // Nothing has happened past the dispatch yet.
    const { rerender } = render(<StreamTimeline run={run([agentStart], 'active')} />)
    expect(screen.getByText('Getting ready')).toBeVisible()
    expect(screen.getByLabelText('Researcher').closest('[data-chat-agent-id]')).toHaveAttribute(
      'data-chat-conversation-id',
      'group-1',
    )

    rerender(<StreamTimeline run={run([agentStart, reasoning], 'active')} />)
    expect(screen.getByText('Thinking')).toBeVisible()

    // The newest event wins: an agent that reasoned and is now in a tool call
    // reports the tool by name rather than replaying its history.
    rerender(<StreamTimeline run={run([agentStart, reasoning, tool], 'active')} />)
    expect(screen.getByText('Running Read')).toBeVisible()

    rerender(<StreamTimeline run={run([agentStart, reasoning, tool, draft], 'active')} />)
    expect(screen.getByText('Writing reply')).toBeVisible()

    // A finished run reports a time, never a phase.
    rerender(<StreamTimeline run={run([agentStart, reasoning, tool, draft], 'completed')} />)
    expect(screen.queryByText('Writing reply')).not.toBeInTheDocument()
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })

  it('reserves the status pill height across a mid-turn phase change', () => {
    const agentStart = event({
      id: 'agent-start:stream-1:agent-1',
      stream_id: 'stream-1',
      type: 'agent_start' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      created_at: '2026-07-16T10:00:01Z',
    })
    const thinking = event({
      id: 'reasoning-1',
      stream_id: 'stream-1',
      type: 'reasoning' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      content: 'Working through it.',
      status: 'streaming' as const,
      created_at: '2026-07-16T10:00:02Z',
    })
    const header = () => screen.getByText('Researcher').parentElement

    const { rerender } = render(
      <StreamTimeline run={run([agentStart, thinking], 'active')} />,
    )
    expect(screen.getByRole('status')).toBeVisible()
    expect(header()).toHaveClass('min-h-5')

    // Between two steps of a live turn nothing is in flight, so the header
    // falls back to a timestamp. That element is shorter than the pill, and
    // without the reserved height the swap resizes the block — which a list
    // pinned to the bottom answers by scrolling the whole backlog.
    rerender(
      <StreamTimeline
        run={run([agentStart, { ...thinking, status: 'done' as const }], 'active')}
      />,
    )
    expect(screen.queryByRole('status')).toBeNull()
    expect(header()).toHaveClass('min-h-5')
  })

  it('stops the waiting timer as soon as an approval is answered', () => {
    const approval = event({
      id: 'approval-1',
      stream_id: 'stream-1',
      type: 'approval_required' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      message: 'Approval required',
      created_at: '2026-07-16T10:00:03Z',
    })
    const events = [
      event({
        id: 'agent-start:stream-1:agent-1',
        stream_id: 'stream-1',
        type: 'agent_start' as const,
        agent_id: 'agent-1',
        display_name: 'Researcher',
        created_at: '2026-07-16T10:00:01Z',
      }),
      event({
        id: 'tool-1',
        stream_id: 'stream-1',
        type: 'tool' as const,
        agent_id: 'agent-1',
        display_name: 'Researcher',
        tool_call_id: 'call-1',
        tool_name: 'Pwsh',
        status: 'approval_required' as const,
        created_at: '2026-07-16T10:00:02Z',
      }),
      approval,
    ]

    const { rerender } = render(<StreamTimeline run={run(events, 'active')} />)
    expect(screen.getByText('Waiting for you')).toBeVisible()

    rerender(
      <StreamTimeline
        run={run([...events.slice(0, -1), { ...approval, approval_resolved: 'approved' }], 'active')}
      />,
    )
    expect(screen.queryByText('Waiting for you')).not.toBeInTheDocument()
    expect(screen.queryByRole('status')).not.toBeInTheDocument()
  })

  it('keeps the checklist out of the reply', () => {
    // The plan renders above the composer; inside the reply it would scroll
    // away while later TodoWrite calls keep changing it.
    const todo = event({
      id: 'todo:stream-1:agent-1',
      stream_id: 'stream-1',
      type: 'todo',
      agent_id: 'agent-1',
      display_name: 'Planner',
      todos: [{ content: 'read the code', status: 'in_progress' as const }],
      created_at: '2026-07-16T10:00:01Z',
    })
    render(<StreamTimeline run={run([todo])} />)

    expect(screen.queryByText('read the code')).toBeNull()
    expect(screen.queryByText('0/1 done')).toBeNull()
  })

  it('localizes an empty active stream state', async () => {
    await i18n.changeLanguage('en-US')
    render(<StreamTimeline run={run([], 'active')} />)
    expect(screen.getByText('Waiting for agents to start…')).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    render(<StreamTimeline run={run([], 'active')} />)
    expect(screen.getByText('等待 Agent 开始…')).toBeVisible()
  })

  it('does not infer moderator scheduling from a finished reasoning segment', () => {
    const reasoning = event({
      id: 'reasoning-1',
      stream_id: 'stream-1',
      type: 'reasoning' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      content: 'Finished one reasoning segment.',
      status: 'done' as const,
      created_at: '2026-07-16T10:00:04Z',
    })
    render(<StreamTimeline moderatorEnabled run={{
      ...run([reasoning], 'active'),
      turn_id: 'turn-1',
      scheduler_status: 'running',
    }} />)

    expect(screen.queryByText('Moderator')).toBeNull()
  })

  it('shows moderator scheduling after an explicit moderator event', () => {
    const finished = event({
      id: 'draft-1',
      stream_id: 'stream-1',
      type: 'response_draft' as const,
      agent_id: 'agent-1',
      display_name: 'Researcher',
      content: 'Finished reply',
      status: 'finalized' as const,
      created_at: '2026-07-16T10:00:04Z',
    })
    render(<StreamTimeline moderatorEnabled run={{
      ...run([finished], 'active'),
      turn_id: 'turn-1',
      scheduler_status: 'running',
      moderator_active: true,
    }} />)

    expect(screen.getByText('Moderator')).toBeVisible()
    expect(screen.getByText('Choosing the next speaker…')).toBeVisible()
    expect(screen.getByText('Finished reply')).toBeVisible()
  })

  it('keeps deterministic scheduler gaps under the generic waiting state', () => {
    render(<StreamTimeline run={{
      ...run([], 'active'),
      turn_id: 'turn-1',
      scheduler_status: 'running',
    }} />)

    expect(screen.queryByText('Moderator')).toBeNull()
    expect(screen.getByText('Waiting for agents to start…')).toBeVisible()
  })

  it('localizes known tool activity statuses', async () => {
    const setupRequired = event({
      id: 'tool-1',
      stream_id: 'stream-1',
      type: 'tool',
      agent_id: 'agent-1',
      display_name: 'Researcher',
      tool_call_id: 'call-1',
      tool_name: 'Configure',
      status: 'setup_required',
      created_at: '2026-07-16T10:00:02Z',
    })

    await i18n.changeLanguage('en-US')
    render(<StreamTimeline run={run([setupRequired])} />)
    expect(screen.getByText('setup required')).toBeInTheDocument()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    render(<StreamTimeline run={run([setupRequired])} />)
    expect(screen.getByText('需要设置')).toBeInTheDocument()
  })

  it('retranslates known stream notices and preserves unknown notice text', async () => {
    const notices = [
      event({
        id: 'silence-1',
        stream_id: 'stream-1',
        type: 'warning',
        message: 'No one replied',
        created_at: '2026-07-16T10:00:01Z',
      }),
      event({
        id: 'warning-1',
        stream_id: 'stream-1',
        type: 'warning',
        message: 'Stream warning',
        created_at: '2026-07-16T10:00:02Z',
      }),
      event({
        id: 'visible-reply-1',
        stream_id: 'stream-1',
        type: 'warning',
        message: 'No visible reply',
        created_at: '2026-07-16T10:00:02Z',
      }),
      event({
        id: 'cancelled-1',
        stream_id: 'stream-1',
        type: 'warning',
        message: 'Stream cancelled',
        created_at: '2026-07-16T10:00:02Z',
      }),
      event({
        id: 'error-1',
        stream_id: 'stream-1',
        type: 'agent_error',
        message: 'Stream failed',
        created_at: '2026-07-16T10:00:03Z',
      }),
      event({
        id: 'waiting-1',
        stream_id: 'stream-1',
        type: 'waiting_for_user',
        message: 'Waiting for your input',
        created_at: '2026-07-16T10:00:04Z',
      }),
      event({
        id: 'unknown-1',
        stream_id: 'stream-1',
        type: 'warning',
        message: 'RAW_STREAM_NOTICE',
        created_at: '2026-07-16T10:00:05Z',
      }),
    ]

    await i18n.changeLanguage('en-US')
    render(<StreamTimeline run={run(notices)} />)
    expect(screen.getByText('No one replied.')).toBeVisible()
    expect(screen.getByText('Stream warning')).toBeVisible()
    expect(screen.getByText('No visible reply')).toBeVisible()
    expect(screen.getByText('Stream cancelled')).toBeVisible()
    expect(screen.getByText('Stream failed')).toBeVisible()
    expect(screen.getByText('Waiting for your input')).toBeVisible()
    expect(screen.getByText('RAW_STREAM_NOTICE')).toBeVisible()

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('无人回复。')).toBeVisible()
    expect(screen.getByText('流警告')).toBeVisible()
    expect(screen.getByText('没有可见回复')).toBeVisible()
    expect(screen.getByText('流已取消')).toBeVisible()
    expect(screen.getByText('流失败')).toBeVisible()
    expect(screen.getByText('等待你的输入')).toBeVisible()
    expect(screen.getByText('RAW_STREAM_NOTICE')).toBeVisible()
  })

  it('retranslates the locally generated unknown tool name without changing other tool names', async () => {
    const user = userEvent.setup()
    const unknownTool = event({
      id: 'tool-unknown', stream_id: 'stream-1', type: 'tool', agent_id: 'agent-1',
      display_name: 'Researcher', tool_call_id: 'call-unknown', tool_name: 'Unknown tool',
      status: 'completed', created_at: '2026-07-16T10:00:02Z',
    })
    await i18n.changeLanguage('en-US')
    render(<StreamTimeline run={run([unknownTool])} />)
    const activity = screen.getByRole('group', { name: 'Activity: 1 tool' })
    await user.click(activity.querySelector(':scope > summary') as HTMLElement)
    expect(screen.getByText('Unknown tool')).toBeVisible()

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('未知工具')).toBeVisible()
  })

  it('names a router dispatch by whether it started or continued a task', async () => {
    const user = userEvent.setup()
    const dispatch = (id: string, status: string) => event({
      id, stream_id: 'stream-1', type: 'tool', agent_id: 'agent-1',
      display_name: 'Lead', tool_call_id: `call-${id}`, tool_name: 'DispatchTask',
      status: 'completed', created_at: '2026-07-16T10:00:02Z',
      result_summary: `{"assistant":"Coder","run":2,"status":"${status}","task_id":"task-1"}`,
    })
    await i18n.changeLanguage('zh-CN')
    render(<StreamTimeline run={run([dispatch('started', 'started'), dispatch('continued', 'continued')])} />)
    const activity = screen.getByRole('group', { name: '活动：2 个工具' })
    await user.click(activity.querySelector(':scope > summary') as HTMLElement)
    expect(screen.getByText('派发任务')).toBeVisible()
    expect(screen.getByText('继续任务')).toBeVisible()
    expect(screen.queryByText('DispatchTask')).not.toBeInTheDocument()
  })

  it('folds live reasoning and tool calls into one activity bubble', async () => {
    const user = userEvent.setup()
    render(
      <StreamTimeline
        run={run([
          event({
            id: 'start-1',
            stream_id: 'stream-1',
            type: 'agent_start',
            agent_id: 'agent-1',
            display_name: 'Researcher',
            created_at: '2026-07-16T10:00:01Z',
          }),
          event({
            id: 'reasoning-1',
            stream_id: 'stream-1',
            type: 'reasoning',
            agent_id: 'agent-1',
            display_name: 'Researcher',
            content: 'Inspect the workspace.',
            status: 'done',
            created_at: '2026-07-16T10:00:02Z',
          }),
          event({
            id: 'reasoning-2',
            stream_id: 'stream-1',
            type: 'reasoning',
            agent_id: 'agent-1',
            display_name: 'Researcher',
            content: 'Compare the relevant files.',
            status: 'done',
            created_at: '2026-07-16T10:00:03Z',
          }),
          event({
            id: 'tool-1',
            stream_id: 'stream-1',
            type: 'tool',
            agent_id: 'agent-1',
            display_name: 'Researcher',
            tool_call_id: 'call-1',
            tool_name: 'Glob',
            status: 'completed',
            args_summary: '*.tsx',
            result_summary: '3 files',
            created_at: '2026-07-16T10:00:04Z',
          }),
          event({
            id: 'tool-2',
            stream_id: 'stream-1',
            type: 'tool',
            agent_id: 'agent-1',
            display_name: 'Researcher',
            tool_call_id: 'call-2',
            tool_name: 'Read',
            status: 'completed',
            created_at: '2026-07-16T10:00:05Z',
          }),
          event({
            id: 'message-2',
            stream_id: 'stream-1',
            type: 'agent_message',
            message_id: 'agent-message-1',
            agent_id: 'agent-1',
            display_name: 'Researcher',
            content: 'Final answer',
            created_at: '2026-07-16T10:00:06Z',
          }),
        ])}
      />,
    )

    const activity = screen.getByRole('group', {
      name: 'Activity: 2 reasoning, 2 tools',
    }) as HTMLDetailsElement
    expect(screen.getAllByRole('group', { name: /activity:/i })).toHaveLength(1)
    expect(activity.open).toBe(false)
    expect(screen.getByText('Final answer')).toBeVisible()

    await user.click(activity.querySelector(':scope > summary') as HTMLElement)
    expect(within(activity).getByText('Inspect the workspace.')).toBeVisible()
    expect(within(activity).getByText('Compare the relevant files.')).toBeVisible()
    expect(within(activity).getByText('Glob')).toBeVisible()
    expect(within(activity).getByText('Read')).toBeVisible()
  })

  it('keeps active work collapsed while exposing its state to assistive technology', () => {
    render(
      <StreamTimeline
        run={run(
          [
            event({
              id: 'reasoning-1',
              stream_id: 'stream-1',
              type: 'reasoning',
              agent_id: 'agent-1',
              display_name: 'Researcher',
              content: 'Still working.',
              status: 'streaming',
              created_at: '2026-07-16T10:00:02Z',
            }),
            event({
              id: 'tool-1',
              stream_id: 'stream-1',
              type: 'tool',
              agent_id: 'agent-1',
              display_name: 'Researcher',
              tool_call_id: 'call-1',
              tool_name: 'Read',
              status: 'started',
              created_at: '2026-07-16T10:00:03Z',
            }),
          ],
          'active',
        )}
      />,
    )

    const activity = screen.getByRole('group', {
      name: 'Activity: 1 reasoning, 1 tool, active',
    }) as HTMLDetailsElement
    expect(activity.open).toBe(false)
  })

  it('settles a finished run whose last activity events never got a terminal status', async () => {
    // A turn that ends without a reply (silence, error, cancellation) leaves its
    // final reasoning/tool events on `streaming`/`started`. The bubble must
    // still read as finished once the run itself is over.
    const events = [
      event({
        id: 'reasoning-1',
        stream_id: 'stream-1',
        type: 'reasoning',
        agent_id: 'agent-1',
        display_name: 'Researcher',
        content: 'Still working.',
        status: 'streaming',
        created_at: '2026-07-16T10:00:02Z',
      }),
      event({
        id: 'tool-1',
        stream_id: 'stream-1',
        type: 'tool',
        agent_id: 'agent-1',
        display_name: 'Researcher',
        tool_call_id: 'call-1',
        tool_name: 'Read',
        status: 'started',
        created_at: '2026-07-16T10:00:03Z',
      }),
      event({
        id: 'silent-1',
        stream_id: 'stream-1',
        type: 'agent_silent',
        agent_id: 'agent-1',
        display_name: 'Researcher',
        message: 'No visible reply',
        created_at: '2026-07-16T10:00:04Z',
      }),
    ]

    await i18n.changeLanguage('en-US')
    render(<StreamTimeline run={run(events, 'completed')} />)
    expect(screen.queryByText('streaming')).not.toBeInTheDocument()
    // The activity bubble stays collapsed, so assert presence rather than
    // visibility: jest-dom reports a closed `details` as not visible.
    const activity = screen.getByRole('group', {
      name: 'Activity: 1 reasoning, 1 tool',
    }) as HTMLDetailsElement
    expect(activity.open).toBe(false)
    expect(screen.getByText('No visible reply')).toBeVisible()
    expect(screen.getByText('No result received')).toBeInTheDocument()
    expect(screen.queryByText('started')).not.toBeInTheDocument()

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('没有可见回复')).toBeVisible()
    expect(screen.getByText('未收到结果')).toBeInTheDocument()
    expect(screen.queryByText('生成中')).not.toBeInTheDocument()
  })

  it('shows approvals after the reply instead of hiding them in activity', () => {
    render(
      <StreamTimeline
        run={run([
          event({
            id: 'tool-1',
            stream_id: 'stream-1',
            type: 'tool',
            agent_id: 'agent-1',
            display_name: 'Assistant',
            tool_call_id: 'call-1',
            tool_name: 'AppPropose',
            status: 'approval_required',
            pending_action: {
              action_id: 'action-1',
              target_kind: 'workspace',
              action: 'create',
              summary: 'Create workspace dsv4-flash',
            },
            created_at: '2026-07-16T10:00:02Z',
          }),
          event({
            id: 'message-1',
            stream_id: 'stream-1',
            type: 'agent_message',
            message_id: 'agent-message-1',
            agent_id: 'agent-1',
            display_name: 'Assistant',
            content: 'Please approve this proposal.',
            created_at: '2026-07-16T10:00:03Z',
          }),
        ])}
      />,
    )

    const activity = screen.getByRole('group', { name: 'Activity: 1 tool' })
    const approval = screen.getByTestId('assistant-approval')
    expect((activity as HTMLDetailsElement).open).toBe(false)
    expect(activity).not.toContainElement(approval)
    expect(approval).toBeVisible()
    expect(screen.getByText('Please approve this proposal.').compareDocumentPosition(approval))
      .toBe(Node.DOCUMENT_POSITION_FOLLOWING)
  })

  it('keeps a required input form visible outside the collapsed activity bubble', () => {
    const inputRequest = {
      question: 'Which environment should I use?',
      required: true,
      choices: ['Staging', 'Production'],
    }
    render(
      <StreamTimeline
        onSubmitHumanInput={vi.fn()}
        run={run(
          [
            event({
              id: 'tool-1',
              stream_id: 'stream-1',
              type: 'tool',
              agent_id: 'agent-1',
              display_name: 'Researcher',
              tool_call_id: 'call-1',
              tool_name: 'AskUser',
              status: 'input_required',
              input_request: inputRequest,
              created_at: '2026-07-16T10:00:02Z',
            }),
            event({
              id: 'waiting-1',
              stream_id: 'stream-1',
              type: 'waiting_for_user',
              agent_id: 'agent-1',
              display_name: 'Researcher',
              message: inputRequest.question,
              input_request: inputRequest,
              created_at: '2026-07-16T10:00:03Z',
            }),
          ],
          'active',
        )}
      />,
    )

    const activity = screen.getByRole('group', { name: 'Activity: 1 tool' }) as HTMLDetailsElement
    expect(activity.open).toBe(false)
    expect(screen.getByText(inputRequest.question)).toBeVisible()
    expect(screen.getByRole('button', { name: 'Staging' })).toBeVisible()
  })

  it('shows an optional question outside the activity bubble when no wait notice follows', () => {
    // An optional AskUser never pauses the turn, so the tool event is the only
    // carrier of the question; it used to render inside the collapsed bubble.
    render(
      <StreamTimeline
        onSubmitHumanInput={vi.fn()}
        run={run(
          [
            event({
              id: 'tool-1',
              stream_id: 'stream-1',
              type: 'tool',
              agent_id: 'agent-1',
              display_name: 'Researcher',
              tool_call_id: 'call-1',
              tool_name: 'AskUser',
              status: 'input_required',
              input_request: { question: 'Any naming preference?', required: false },
              created_at: '2026-07-16T10:00:02Z',
            }),
          ],
          'active',
        )}
      />,
    )

    const activity = screen.getByRole('group', { name: 'Activity: 1 tool' })
    const question = screen.getByText('Any naming preference?')
    expect((activity as HTMLDetailsElement).open).toBe(false)
    expect(activity).not.toContainElement(question)
    expect(question).toBeVisible()
    expect(screen.getByRole('textbox')).toBeVisible()
  })
})
