import { beforeEach, describe, expect, it } from 'vitest'

import {
  clearTerminalMetadata,
  loadTerminalMetadata,
  saveTerminalMetadata,
  TERMINAL_METADATA_STORAGE_KEY,
  type TerminalMetadataStore,
} from './metadataStore'

describe('terminal metadata storage', () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it('persists only restorable metadata and drops sensitive runtime fields', () => {
    const unsafeValue = {
      height: 320,
      sessionId: 'root-session-secret',
      output: 'root-output-secret',
      env: { TOKEN: 'root-env-secret' },
      history: ['root-history-secret'],
      conversations: {
        'chat-1': {
          open: true,
          activeTabId: 'tab-1',
          sessionId: 'session-secret',
          output: 'output-secret',
          env: { TOKEN: 'env-secret' },
          history: ['history-secret'],
          tabs: [
            {
              id: 'tab-1',
              label: 'PowerShell',
              launchDirectory: 'D:/project',
              sessionId: 'tab-session-secret',
              output: 'tab-output-secret',
              env: 'tab-env-secret',
              history: 'tab-history-secret',
            },
          ],
        },
      },
    } as unknown as TerminalMetadataStore

    saveTerminalMetadata(unsafeValue)

    const raw = localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY) ?? ''
    expect(JSON.parse(raw)).toEqual({
      height: 320,
      conversations: {
        'chat-1': {
          open: true,
          activeTabId: 'tab-1',
          tabs: [
            { id: 'tab-1', label: 'PowerShell', launchDirectory: 'D:/project' },
          ],
        },
      },
    })
    for (const forbidden of ['sessionId', 'output', 'env', 'history', 'secret']) {
      expect(raw).not.toContain(forbidden)
    }
    expect(loadTerminalMetadata().conversations['chat-1']?.activeTabId).toBe('tab-1')
  })

  it('falls back to an empty schema for invalid JSON and invalid roots', () => {
    localStorage.setItem(TERMINAL_METADATA_STORAGE_KEY, '{broken')
    expect(loadTerminalMetadata()).toEqual({ height: 0, conversations: {} })

    for (const invalid of [null, [], 'metadata', 42]) {
      localStorage.setItem(TERMINAL_METADATA_STORAGE_KEY, JSON.stringify(invalid))
      expect(loadTerminalMetadata()).toEqual({ height: 0, conversations: {} })
    }
  })

  it('validates and cleans every persisted field independently', () => {
    localStorage.setItem(
      TERMINAL_METADATA_STORAGE_KEY,
      JSON.stringify({
        height: '320',
        ignoredRoot: true,
        conversations: {
          'chat-1': {
            open: 'yes',
            activeTabId: 7,
            ignoredConversation: true,
            tabs: [
              {
                id: 'tab-1',
                label: 'PowerShell',
                launchDirectory: 'D:/project',
                ignoredTab: true,
              },
              { id: 'missing-directory', label: 'Invalid' },
              null,
            ],
          },
          'chat-invalid': null,
        },
      }),
    )

    expect(loadTerminalMetadata()).toEqual({
      height: 0,
      conversations: {
        'chat-1': {
          open: false,
          activeTabId: null,
          tabs: [
            { id: 'tab-1', label: 'PowerShell', launchDirectory: 'D:/project' },
          ],
        },
      },
    })
  })

  it('keeps a finite height for viewport-aware clamping later', () => {
    localStorage.setItem(
      TERMINAL_METADATA_STORAGE_KEY,
      JSON.stringify({ height: 50_000, conversations: {} }),
    )
    expect(loadTerminalMetadata().height).toBe(50_000)
  })

  it('clears the versioned metadata key', () => {
    saveTerminalMetadata({ height: 240, conversations: {} })
    expect(localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY)).not.toBeNull()

    clearTerminalMetadata()

    expect(localStorage.getItem(TERMINAL_METADATA_STORAGE_KEY)).toBeNull()
    expect(loadTerminalMetadata()).toEqual({ height: 0, conversations: {} })
  })
})
