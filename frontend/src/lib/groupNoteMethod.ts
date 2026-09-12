import type { TFunction } from 'i18next'

export const noteStatuses = ['proposed', 'implemented', 'rejected', 'archived'] as const
export const noteCategories = ['决策', '约定', '踩坑'] as const
export type NoteStatus = typeof noteStatuses[number]
export type NoteCategory = typeof noteCategories[number]

// Note: 元数据就地编辑，避免与 Agent 修改的正文分叉 — 见 .agents/notes/implemented/feature/2026-09-12-built-in-group-note-method.md。
export function noteMetadata(content: string) {
  const match = /^# [^\r\n]*\r?\nStatus: (proposed|implemented|rejected|archived)(?: — ([^\r\n]*))?\r?\nSince: (\d{4}-\d{2}-\d{2})\r?\nCategory: (决策|约定|踩坑)(?:\r?\n|$)/.exec(content)
  if (!match) return null
  return { status: match[1] as NoteStatus, reason: match[2] ?? '', since: match[3], category: match[4] as NoteCategory }
}

export function setNoteMetadata(content: string, field: 'Status' | 'Category', value: string) {
  if (!noteMetadata(content)) return content
  // Only touch the header, even when a body example contains the same field.
  const lines = content.split('\n')
  const index = field === 'Status' ? 1 : 3
  const ending = lines[index].endsWith('\r') ? '\r' : ''
  lines[index] = `${field}: ${value.replace(/[\r\n]/g, ' ')}${ending}`
  return lines.join('\n')
}

export function setNoteTitle(content: string, title: string) {
  if (!noteMetadata(content)) return content
  return content.replace(/^# [^\r\n]*/, () => `# ${title.replace(/[\r\n]/g, ' ')}`)
}

export function newGroupNoteContent(title: string, t: TFunction, today = new Date()) {
  const since = [today.getFullYear(), String(today.getMonth() + 1).padStart(2, '0'), String(today.getDate()).padStart(2, '0')].join('-')
  return `# ${title}\nStatus: proposed\nSince: ${since}\nCategory: 决策\n\n## Problem\n\n${t('chat:workspace.notesPanel.problemPrompt')}\n\n## Decision\n\n${t('chat:workspace.notesPanel.decisionPrompt')}\n\n## Alternatives considered\n\n${t('chat:workspace.notesPanel.alternativesPrompt')}\n\n## Consequences\n\n${t('chat:workspace.notesPanel.consequencesPrompt')}\n`
}
