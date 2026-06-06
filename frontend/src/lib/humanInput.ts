export interface HumanInputRequest {
  question: string
  required?: boolean
}

const HUMAN_INPUT_PREFIX = 'Human input requested:'

function stripHumanInputPrefix(value: string): string | null {
  const index = value.toLocaleLowerCase().indexOf(HUMAN_INPUT_PREFIX.toLocaleLowerCase())
  if (index === -1) return null
  const question = value.slice(index + HUMAN_INPUT_PREFIX.length).trim()
  return question || null
}

function unescapeSummaryValue(value: string): string {
  return value
    .replace(/\\n/g, '\n')
    .replace(/\\r/g, '\r')
    .replace(/\\t/g, '\t')
    .replace(/\\'/g, "'")
    .replace(/\\"/g, '"')
    .trim()
}

function questionFromArgsSummary(value: string | undefined): HumanInputRequest | null {
  if (!value) return null
  const singleQuoted = value.match(/question='((?:\\'|[^'])*)'/)
  const doubleQuoted = value.match(/question="((?:\\"|[^"])*)"/)
  const rawQuestion = singleQuoted?.[1] ?? doubleQuoted?.[1]
  if (!rawQuestion) return null
  const required = !/\brequired=False\b/.test(value)
  return {
    question: unescapeSummaryValue(rawQuestion),
    required,
  }
}

export function humanInputRequestFromText(value: string | null | undefined): HumanInputRequest | null {
  if (!value) return null
  const question = stripHumanInputPrefix(value)
  return question ? { question, required: true } : null
}

export function normalizeHumanInputRequest(
  value: HumanInputRequest | null | undefined,
  fallbackText?: string | null,
  argsSummary?: string,
): HumanInputRequest | null {
  if (value?.question?.trim()) {
    const question = stripHumanInputPrefix(value.question) ?? value.question.trim()
    return {
      question,
      required: value.required,
    }
  }
  return humanInputRequestFromText(fallbackText) ?? questionFromArgsSummary(argsSummary)
}

export function formatHumanInputResponse(answer: string, targetDisplayName?: string): string {
  const trimmed = answer.trim()
  const target = targetDisplayName?.trim()
  if (!target || target === 'Agent' || target === 'Assistant') {
    return trimmed
  }
  return `@${target} ${trimmed}`
}
