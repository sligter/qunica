import { beforeEach, describe, expect, it } from 'vitest'

import {
  selectConversationStatus,
  selectThreadStatus,
  useConversationActivityStore,
} from '@/stores/conversationActivityStore'

const initialState = useConversationActivityStore.getInitialState()

function store() {
  return useConversationActivityStore.getState()
}

function startRun(id: string, threadId: string | null = 'thread-1') {
  store().startRun({ id, conversationId: 'group-1', threadId, scope: 'groups' })
}

function conversationStatus() {
  return selectConversationStatus(store(), 'group-1')
}

function threadStatus(threadId: string) {
  return selectThreadStatus(store(), 'group-1', threadId)
}

describe('conversationActivityStore', () => {
  beforeEach(() => {
    useConversationActivityStore.setState(initialState, true)
  })

  it('reports a running conversation and thread while a run is open', () => {
    expect(conversationStatus()).toBeNull()

    startRun('run-1')

    expect(conversationStatus()).toBe('running')
    expect(threadStatus('thread-1')).toBe('running')
    expect(threadStatus('thread-2')).toBeNull()
  })

  it('clears the status when a run completes without pausing', () => {
    startRun('run-1')

    const finished = store().finishRun('run-1', 'completed')

    expect(finished?.id).toBe('run-1')
    expect(finished?.announced).toBe(false)
    expect(conversationStatus()).toBeNull()
  })

  it('announces a pause once and keeps it after the stream closes', () => {
    startRun('run-1')

    expect(store().markRunWaiting('run-1')?.id).toBe('run-1')
    expect(store().markRunWaiting('run-1')).toBeNull()
    expect(conversationStatus()).toBe('waiting')

    // The stream ends cleanly while the question is still open.
    const finished = store().finishRun('run-1', 'completed')

    expect(finished?.announced).toBe(true)
    expect(threadStatus('thread-1')).toBe('waiting')
  })

  it('carries a failure only when the user was looking elsewhere', () => {
    startRun('run-1')
    store().finishRun('run-1', 'failed', 'boom')

    expect(threadStatus('thread-1')).toBe('failed')

    store().clearFailure('group-1', 'thread-1')
    expect(threadStatus('thread-1')).toBeNull()

    store().setViewedConversation('group-1', 'thread-1')
    startRun('run-2')
    store().finishRun('run-2', 'failed', 'boom')

    expect(threadStatus('thread-1')).toBeNull()
  })

  it('leaves a pending question alone when a failure is cleared', () => {
    startRun('run-1')
    store().markRunWaiting('run-1')
    store().finishRun('run-1', 'completed')

    store().clearFailure('group-1', 'thread-1')

    expect(threadStatus('thread-1')).toBe('waiting')
  })

  it('drops the pending state when the next message answers it', () => {
    startRun('run-1')
    store().markRunWaiting('run-1')
    store().finishRun('run-1', 'completed')
    expect(threadStatus('thread-1')).toBe('waiting')

    startRun('run-2')

    expect(threadStatus('thread-1')).toBe('running')
  })

  it('leaves nothing behind when a run is cancelled', () => {
    startRun('run-1')
    store().markRunWaiting('run-1')

    store().finishRun('run-1', 'cancelled')

    expect(threadStatus('thread-1')).toBeNull()
  })

  it('rolls every task up to the conversation, worst first', () => {
    startRun('run-1', 'thread-1')
    startRun('run-2', 'thread-2')

    expect(conversationStatus()).toBe('running')

    store().markRunWaiting('run-2')

    expect(conversationStatus()).toBe('waiting')
    expect(threadStatus('thread-1')).toBe('running')
    expect(threadStatus('thread-2')).toBe('waiting')
  })

  it('names a run from the titles the chat view registered', () => {
    store().registerConversationTitles('group-1', 'thread-1', {
      conversation: 'Platform',
      thread: 'Ship the API',
    })

    startRun('run-1')

    const run = store().runs['run-1']
    expect(run?.conversation_title).toBe('Platform')
    expect(run?.thread_title).toBe('Ship the API')
  })

  it('only forgets the viewed conversation when it is the one that left', () => {
    store().setViewedConversation('group-1', 'thread-1')

    store().clearViewedConversation('group-2')
    expect(store().viewed).toEqual({ conversation_id: 'group-1', thread_id: 'thread-1' })

    store().clearViewedConversation('group-1')
    expect(store().viewed).toBeNull()
  })
})
