import { useQuery } from '@tanstack/react-query'

import { fetchJson, ApiError } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  AcpPermissionPolicy,
  AcpRuntimeCapabilitiesRead,
  AcpRuntimeProfile,
} from '@/types/api'

export interface AcpRuntimeCapabilitiesInput {
  profile: AcpRuntimeProfile
  command: string
  args: string[]
  env: Record<string, string>
  permission_policy: AcpPermissionPolicy
  selected_model?: string | null
}

function normalizedInput(input: AcpRuntimeCapabilitiesInput) {
  const selectedModel = input.selected_model?.trim() ?? ''
  return {
    profile: input.profile,
    command: input.command.trim(),
    args: input.args,
    env: input.env,
    permission_policy: input.permission_policy,
    model: selectedModel || null,
  }
}

function fingerprint(value: unknown): string {
  const text = JSON.stringify(value)
  let hash = 2166136261
  for (let index = 0; index < text.length; index += 1) {
    hash ^= text.charCodeAt(index)
    hash = Math.imul(hash, 16777619)
  }
  return (hash >>> 0).toString(36)
}

function capabilityResponseFromError(error: unknown): AcpRuntimeCapabilitiesRead | null {
  if (!(error instanceof ApiError)) return null
  try {
    const parsed = JSON.parse(error.message) as Partial<AcpRuntimeCapabilitiesRead>
    if (parsed.source !== 'acp' || typeof parsed.warning !== 'string') return null
    return {
      models: Array.isArray(parsed.models) ? parsed.models : [],
      modes: Array.isArray(parsed.modes) ? parsed.modes : [],
      thinking_efforts: Array.isArray(parsed.thinking_efforts)
        ? parsed.thinking_efforts
        : [],
      current_model: parsed.current_model ?? null,
      current_mode: parsed.current_mode ?? null,
      current_thinking_effort: parsed.current_thinking_effort ?? null,
      source: 'acp',
      warning: parsed.warning,
    }
  } catch {
    return null
  }
}

export function acpRuntimeCapabilitiesQueryKey(
  input: AcpRuntimeCapabilitiesInput | null,
) {
  const request = input ? normalizedInput(input) : null
  return [
    'agents',
    'acp-runtime-capabilities',
    request ? fingerprint(request) : 'disabled',
  ] as const
}

export function useAcpRuntimeCapabilities(
  input: AcpRuntimeCapabilitiesInput | null,
  enabled: boolean,
) {
  const token = useAuthStore((state) => state.token)
  const request = input ? normalizedInput(input) : null

  return useQuery({
    queryKey: acpRuntimeCapabilitiesQueryKey(input),
    queryFn: async ({ signal }) => {
      try {
        return await fetchJson<AcpRuntimeCapabilitiesRead>(
          '/agents/acp-runtime-capabilities',
          {
            method: 'POST',
            token,
            body: request,
            signal,
          },
        )
      } catch (error) {
        const response = capabilityResponseFromError(error)
        if (response) return response
        throw error
      }
    },
    enabled: token !== null && enabled && request !== null,
    staleTime: 5 * 60 * 1000,
    retry: false,
  })
}
