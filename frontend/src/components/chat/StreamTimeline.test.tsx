import { cleanup, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { StreamTimeline } from '@/components/chat/StreamTimeline'
import type { StreamRun, StreamTimelineEvent } from '@/stores/messageStore'

vi.mock('@/hooks/useGroupAgents', () => ({
  useGroupAgents: () => ({ data: [] }),
}))

vi.mock('@/components/chat/MarkdownMessage', () => ({
  MarkdownMessage: ({ content }: { content: string }) => <div>{content}</div>,
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

afterEach(cleanup)

describe('StreamTimeline activity rendering', () => {
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
