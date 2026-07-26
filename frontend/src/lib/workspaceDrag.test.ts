import { describe, expect, it } from 'vitest'

import {
  WORKSPACE_DRAG_ITEM_VERSION,
  WORKSPACE_ITEM_MIME,
  decodeWorkspaceDragItem,
  encodeWorkspaceDragItem,
  encodeWorkspaceDragItems,
  workspaceItemsFromDataTransfer,
  workspacePathsFromDataTransfer,
} from './workspaceDrag'

function dataTransfer(values: Record<string, string>): DataTransfer {
  return {
    getData: (type: string) => values[type] ?? '',
  } as DataTransfer
}

describe('workspace drag item protocol', () => {
  it('encodes and decodes a versioned workspace item', () => {
    const encoded = encodeWorkspaceDragItem({
      path: 'docs/guide.md',
      name: 'guide.md',
      kind: 'file',
    })

    expect(JSON.parse(encoded)).toEqual({
      version: WORKSPACE_DRAG_ITEM_VERSION,
      path: 'docs/guide.md',
      name: 'guide.md',
      kind: 'file',
    })
    expect(decodeWorkspaceDragItem(encoded)).toEqual({
      version: WORKSPACE_DRAG_ITEM_VERSION,
      path: 'docs/guide.md',
      name: 'guide.md',
      kind: 'file',
    })
  })

  it.each([
    ['invalid JSON', '{'],
    ['an array instead of one item', '[]'],
    ['an unsupported version', JSON.stringify({ version: 2, path: 'a.txt', name: 'a.txt', kind: 'file' })],
    ['an extra field', JSON.stringify({ version: 1, path: 'a.txt', name: 'a.txt', kind: 'file', trusted: true })],
    ['an absolute path', JSON.stringify({ version: 1, path: '/a.txt', name: 'a.txt', kind: 'file' })],
    ['a traversal path', JSON.stringify({ version: 1, path: '../a.txt', name: 'a.txt', kind: 'file' })],
    ['a Windows path', JSON.stringify({ version: 1, path: 'C:\\a.txt', name: 'a.txt', kind: 'file' })],
    ['an empty path segment', JSON.stringify({ version: 1, path: 'docs//a.txt', name: 'a.txt', kind: 'file' })],
    ['a mismatched name', JSON.stringify({ version: 1, path: 'docs/a.txt', name: 'b.txt', kind: 'file' })],
    ['an unknown kind', JSON.stringify({ version: 1, path: 'a.txt', name: 'a.txt', kind: 'link' })],
  ])('rejects %s', (_label, raw) => {
    expect(decodeWorkspaceDragItem(raw)).toBeNull()
  })

  it('rejects invalid items before encoding them', () => {
    expect(() => encodeWorkspaceDragItem({
      path: '../secret.txt',
      name: 'secret.txt',
      kind: 'file',
    })).toThrow(TypeError)
  })

  it('filters invalid entries, deduplicates paths, and classifies files and directories', () => {
    const validBatch = JSON.parse(encodeWorkspaceDragItems([
      { path: 'docs', name: 'docs', kind: 'directory' },
      { path: 'docs/guide.md', name: 'guide.md', kind: 'file' },
      { path: 'docs/guide.md', name: 'guide.md', kind: 'file' },
    ])) as unknown[]
    validBatch.splice(1, 0,
      { version: 1, path: '', name: '', kind: 'file' },
      { version: 1, path: 'escape/../secret.txt', name: 'secret.txt', kind: 'file' },
    )

    const result = workspaceItemsFromDataTransfer(dataTransfer({
      [WORKSPACE_ITEM_MIME]: JSON.stringify(validBatch),
    }))

    expect(result.directories.map((item) => item.path)).toEqual(['docs'])
    expect(result.files.map((item) => item.path)).toEqual(['docs/guide.md'])
  })

  it('reads a single encoded item from the custom DataTransfer MIME', () => {
    const encoded = encodeWorkspaceDragItem({
      path: 'images/photo.png',
      name: 'photo.png',
      kind: 'file',
    })

    expect(workspaceItemsFromDataTransfer(dataTransfer({
      [WORKSPACE_ITEM_MIME]: encoded,
    }))).toEqual({
      files: [{
        version: WORKSPACE_DRAG_ITEM_VERSION,
        path: 'images/photo.png',
        name: 'photo.png',
        kind: 'file',
      }],
      directories: [],
    })
  })

  it('does not trust text/plain as a workspace item', () => {
    const forged = encodeWorkspaceDragItem({
      path: 'secrets.txt',
      name: 'secrets.txt',
      kind: 'file',
    })
    const transfer = dataTransfer({ 'text/plain': forged })

    expect(workspaceItemsFromDataTransfer(transfer)).toEqual({ files: [], directories: [] })
  })

  it('retains the legacy text/plain path reader for insert-path behavior', () => {
    const transfer = dataTransfer({ 'text/plain': 'docs/guide.md\nassets' })

    expect(workspacePathsFromDataTransfer(transfer)).toEqual(['docs/guide.md', 'assets'])
  })

  it('rejects unsafe paths and bounds text/plain drop candidates', () => {
    const valid = Array.from({ length: 12 }, (_, index) => `docs/file-${index}.md`)
    const transfer = dataTransfer({
      'text/plain': [
        ...valid,
        '../secret.txt',
        '/absolute.txt',
        'a'.repeat(1025),
      ].join('\n'),
    })

    expect(workspacePathsFromDataTransfer(transfer)).toEqual(valid.slice(0, 10))
  })

  it('rejects oversized text/plain drop payloads', () => {
    const transfer = dataTransfer({ 'text/plain': 'a'.repeat(16 * 1024 + 1) })

    expect(workspacePathsFromDataTransfer(transfer)).toEqual([])
  })
})
