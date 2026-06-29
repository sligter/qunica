import { useQuery } from '@tanstack/react-query'

import { fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type { AcpRuntimePresetListResponse, AcpRuntimePresetRead } from '@/types/api'

export const FALLBACK_ACP_RUNTIME_PRESETS = [
  {
    id: 'codex',
    name: 'Codex',
    description: 'Codex CLI through the Zed Codex ACP adapter.',
    profile: 'codex',
    installed: false,
    command: 'npx',
    args: ['@zed-industries/codex-acp'],
    env: {},
    timeout_seconds: 3600,
    permission_policy: 'deny',
    default_model: null,
    default_mode: 'read-only',
    default_thinking_effort: 'medium',
    model_options: [],
    mode_options: [
      {
        value: 'read-only',
        label: 'Read Only',
        description: 'Read files in the current workspace; ask before edits or internet.',
      },
      {
        value: 'auto',
        label: 'Default',
        description: 'Read and edit workspace files; ask for internet or external edits.',
      },
      {
        value: 'full-access',
        label: 'Full Access',
        description: 'Edit outside the workspace and access the internet without asking.',
      },
    ],
    thinking_effort_options: [
      { value: '', label: 'Default' },
      { value: 'minimal', label: 'Minimal' },
      { value: 'low', label: 'Low' },
      { value: 'medium', label: 'Medium' },
      { value: 'high', label: 'High' },
      { value: 'xhigh', label: 'XHigh' },
    ],
    install_hint:
      'Install @zed-industries/codex-acp so codex-acp is on PATH, or keep the npx fallback command.',
    source: 'fallback',
  },
  {
    id: 'claude',
    name: 'Claude Code',
    description: 'Claude Agent SDK through the official Claude Agent ACP adapter.',
    profile: 'claude',
    installed: false,
    command: 'npx',
    args: ['@agentclientprotocol/claude-agent-acp'],
    env: {},
    timeout_seconds: 3600,
    permission_policy: 'deny',
    default_model: null,
    default_mode: 'default',
    default_thinking_effort: 'high',
    model_options: [],
    mode_options: [
      { value: 'default', label: 'Default' },
      {
        value: 'auto',
        label: 'Auto',
        description: 'Use a model classifier to approve or deny permission prompts.',
      },
      { value: 'acceptEdits', label: 'Accept Edits' },
      { value: 'plan', label: 'Plan' },
      { value: 'dontAsk', label: "Don't Ask" },
      { value: 'bypassPermissions', label: 'Bypass Permissions' },
    ],
    thinking_effort_options: [
      { value: 'low', label: 'Low' },
      { value: 'medium', label: 'Medium' },
      { value: 'high', label: 'High' },
      { value: 'max', label: 'Max' },
    ],
    install_hint:
      'Install @agentclientprotocol/claude-agent-acp so claude-agent-acp is on PATH, or keep the npx fallback command.',
    source: 'fallback',
  },
] satisfies AcpRuntimePresetRead[]

const fallbackResponse = { presets: FALLBACK_ACP_RUNTIME_PRESETS }

function mergePreset(
  preset: AcpRuntimePresetRead | undefined,
  fallback: AcpRuntimePresetRead,
): AcpRuntimePresetRead {
  if (!preset) return fallback
  const hasCommand = typeof preset.command === 'string' && preset.command.trim() !== ''
  return {
    ...fallback,
    ...preset,
    command: hasCommand ? preset.command : fallback.command,
    args: preset.args.length > 0 || preset.installed ? preset.args : fallback.args,
    env: preset.env ?? fallback.env,
    model_options:
      preset.model_options.length > 0 ? preset.model_options : fallback.model_options,
    mode_options:
      preset.mode_options.length > 0 ? preset.mode_options : fallback.mode_options,
    thinking_effort_options:
      preset.thinking_effort_options.length > 0
        ? preset.thinking_effort_options
        : fallback.thinking_effort_options,
    install_hint: preset.install_hint || fallback.install_hint,
    source: preset.source ?? fallback.source,
  }
}

export function normalizeAcpRuntimePresetResponse(
  response: AcpRuntimePresetListResponse | null | undefined,
): AcpRuntimePresetListResponse {
  const serverPresets = response?.presets ?? []
  const serverById = new Map(serverPresets.map((preset) => [preset.id, preset]))
  const fallbackIds = new Set(FALLBACK_ACP_RUNTIME_PRESETS.map((preset) => preset.id))
  const presets = [
    ...FALLBACK_ACP_RUNTIME_PRESETS.map((fallback) =>
      mergePreset(serverById.get(fallback.id), fallback),
    ),
    ...serverPresets.filter((preset) => !fallbackIds.has(preset.id)),
  ]
  return { presets }
}

export function useAcpRuntimePresets() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['agents', 'acp-runtime-presets'],
    queryFn: async () => {
      try {
        return normalizeAcpRuntimePresetResponse(
          await fetchJson<AcpRuntimePresetListResponse>('/agents/acp-runtime-presets', {
            token,
          }),
        )
      } catch {
        return fallbackResponse
      }
    },
    enabled: token !== null,
    placeholderData: fallbackResponse,
  })
}
