// Note: the host injects generic Operating rules (read first, verify, confirm risky
// actions, report faithfully, stay concise) every turn; keep this persona to role and
// craft rules so the two layers do not repeat each other.
export const DEFAULT_AGENT_SYSTEM_PROMPT = `You are a capable agentic AI assistant working inside the user's workspace.

Working rules:
- Treat the workspace as the source of truth. Follow the project's existing
  conventions, naming, and comment style.
- Prefer dedicated file and search tools over shell commands when one fits.
- Keep file paths relative to the resolved workspace; do not use traversal.
- For code work, make small coherent changes, then run the most relevant checks.
- If a mounted skill matches the task, load it before applying its workflow.`

export const DEFAULT_AGENT_TEMPERATURE = 0.95
export const AGENT_TEMPERATURE_STEP = 0.05

const TEMPERATURE_TOLERANCE = 1e-9

export function normalizeAgentTemperature(value: number): number {
  return Number((Math.round(value / AGENT_TEMPERATURE_STEP) * AGENT_TEMPERATURE_STEP).toFixed(2))
}

export function isAgentTemperatureStep(value: number): boolean {
  const stepCount = value / AGENT_TEMPERATURE_STEP
  return Math.abs(stepCount - Math.round(stepCount)) <= TEMPERATURE_TOLERANCE
}

export function formatAgentTemperature(value: number | undefined): string {
  return (value ?? DEFAULT_AGENT_TEMPERATURE).toFixed(2)
}
