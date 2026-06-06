export interface HumanInputRequest {
  question: string
  required?: boolean
  input_type?: 'text' | 'choice'
  choices?: string[]
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
  const choices = choicesFromArgsSummary(value)
  return {
    question: unescapeSummaryValue(rawQuestion),
    required,
    choices: choices.length > 0 ? choices : undefined,
  }
}

function choicesFromArgsSummary(value: string): string[] {
  const choicesMatch = value.match(/choices=\[((?:.|\n)*?)\]/)
  const choicesBody = choicesMatch?.[1]
  if (!choicesBody) return []

  const choices: string[] = []
  const quotedValuePattern = /'((?:\\'|[^'])*)'|"((?:\\"|[^"])*)"/g
  for (const match of choicesBody.matchAll(quotedValuePattern)) {
    const rawChoice = match[1] ?? match[2]
    const choice = unescapeSummaryValue(rawChoice)
    if (choice) choices.push(choice)
    if (choices.length >= 8) break
  }
  return choices
}

function normalizeChoices(value: string[] | undefined): string[] | undefined {
  const choices = (value ?? [])
    .map((choice) => choice.trim())
    .filter(Boolean)
    .slice(0, 8)
  return choices.length > 0 ? choices : undefined
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
    const choices = normalizeChoices(value.choices)
    return {
      question,
      required: value.required,
      input_type: choices ? 'choice' : value.input_type,
      choices,
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
