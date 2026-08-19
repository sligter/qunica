export const thinkingLevelValues = ['default', 'low', 'medium', 'high', 'xhigh', 'max'] as const

export type ThinkingLevel = (typeof thinkingLevelValues)[number]

export const thinkingLevelOptions: { value: ThinkingLevel; label: string }[] = [
  { value: 'default', label: 'Default' },
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' },
  { value: 'xhigh', label: 'XHigh' },
  { value: 'max', label: 'Max' },
]

export function isThinkingLevel(value: unknown): value is ThinkingLevel {
  return thinkingLevelValues.some((option) => option === value)
}
