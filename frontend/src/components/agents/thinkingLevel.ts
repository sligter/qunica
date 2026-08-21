export const thinkingLevelValues = ['default', 'low', 'medium', 'high', 'xhigh', 'max'] as const

export type ThinkingLevel = (typeof thinkingLevelValues)[number]

export function isThinkingLevel(value: unknown): value is ThinkingLevel {
  return thinkingLevelValues.some((option) => option === value)
}
