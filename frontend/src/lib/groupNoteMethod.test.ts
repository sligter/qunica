import { describe, expect, it } from 'vitest'
import i18n from '@/i18n'
import { newGroupNoteContent, noteMetadata, setNoteMetadata, setNoteTitle } from './groupNoteMethod'

describe('group note method', () => {
  it('creates a dated proposal with the four sections in either language', () => {
    for (const locale of ['zh-CN', 'en-US']) {
      const content = newGroupNoteContent('Decision', i18n.getFixedT(locale), new Date(2026, 8, 12))
      expect(noteMetadata(content)).toEqual({ status: 'proposed', reason: '', since: '2026-09-12', category: '决策' })
      expect(content.match(/^## .+$/gm)).toEqual(['## Problem', '## Decision', '## Alternatives considered', '## Consequences'])
      expect(content).toContain(locale === 'zh-CN' ? '不做 / 复用现有方案' : 'Do nothing / reuse the existing solution')
    }
  })

  it('edits only header metadata and preserves the original date and body verbatim', () => {
    const original = '# Old\r\nStatus: proposed\r\nSince: 2025-01-02\r\nCategory: 决策\r\n\r\n## Decision\r\nStatus: proposed\r\n$& body\r\n'
    let edited = setNoteMetadata(original, 'Status', 'rejected — Too costly')
    edited = setNoteMetadata(edited, 'Category', '踩坑')
    edited = setNoteTitle(edited, '$& New')
    expect(edited).toBe('# $& New\r\nStatus: rejected — Too costly\r\nSince: 2025-01-02\r\nCategory: 踩坑\r\n\r\n## Decision\r\nStatus: proposed\r\n$& body\r\n')
    expect(noteMetadata(edited)).toEqual({ status: 'rejected', reason: 'Too costly', since: '2025-01-02', category: '踩坑' })
  })

  it('leaves free-form notes and header examples untouched', () => {
    const legacy = 'Original text\n\n# Example\nStatus: proposed\nSince: 2026-09-12\nCategory: 决策\n'
    expect(noteMetadata(legacy)).toBeNull()
    expect(setNoteTitle(legacy, 'New title')).toBe(legacy)
    expect(setNoteMetadata(legacy, 'Status', 'implemented')).toBe(legacy)
  })
})
