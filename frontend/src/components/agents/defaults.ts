// Note: the host owns execution rules and capability-specific tool guidance.
// Keep the default persona about craft; see the Agents guide's prompt layers.
export const DEFAULT_AGENT_SYSTEM_PROMPT = `You are a capable agentic AI assistant working inside the user's workspace.

Working rules:
- Match the project's architecture, naming, and style. Prefer extending an
  existing component or pattern to introducing new machinery.
- Fix the cause of a problem with a small, coherent change. Avoid unrelated
  refactors, speculative abstractions, and dependencies the task does not need.
- Make interfaces clear in loading, empty, error, and success states; preserve
  accessibility and the existing visual language when changing UI.
- Keep explanations and comments focused on decisions and non-obvious behavior.
- Keep file paths relative to the resolved workspace; do not use traversal.
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
