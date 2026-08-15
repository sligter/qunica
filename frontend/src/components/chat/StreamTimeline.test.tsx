import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { StreamTimeline } from '@/components/chat/StreamTimeline'
import i18n from '@/i18n'
import type { StreamRun, StreamTimelineEvent } from '@/stores/messageStore'

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
    terminal_reason: null,
    criticalSummaries: [],
    created_at: '2026-07-16T10:00:00Z',
    updated_at: '2026-07-16T10:00:06Z',
    events,
  }
}

afterEach(async () => {
  cleanup()
  await i18n.changeLanguage('en-US')
})

describe('StreamTimeline activity rendering', () => {
  it('localizes an empty active stream state', async () => {
    await i18n.changeLanguage('en-US')
    render(<StreamTimeline run={run([], 'active')} />)
    expect(screen.getByText('Waiting for agents to start…')).toBeVisible()

    cleanup()
    await i18n.changeLanguage('zh-CN')
    render(<StreamTimeline run={run([], 'active')} />)
    expect(screen.getByText('等待 Agent 开始…')).toBeVisible()
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

    await i18n.changeLanguage('zh-CN')
    expect(await screen.findByText('没有可见回复')).toBeVisible()
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
})
