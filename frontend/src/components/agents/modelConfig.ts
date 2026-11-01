// Note: editing visible controls must preserve engine limits and future model
// settings. Only fields owned by the form are normalized or removed.
export function mergeModelConfig(previous: Record<string, unknown> | null,
  edited: Record<string, unknown>): Record<string, unknown> {
  const merged = { ...previous }
  for (const key of ['model', 'vision', 'temperature', 'top_p', 'reasoning_effort',
    'context_window_tokens', 'context_output_reserve_ratio']) delete merged[key]
  Object.assign(merged, edited)
  merged.engine = 'rig'
  return merged
}
