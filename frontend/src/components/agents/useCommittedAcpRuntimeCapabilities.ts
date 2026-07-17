import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'

import { parseAcpArgs, parseAcpEnv } from '@/components/agents/acpRuntimeConfig'
import {
  acpRuntimeCapabilitiesQueryKey,
  useAcpRuntimeCapabilities,
  type AcpRuntimeCapabilitiesInput,
} from '@/hooks/useAcpRuntimeCapabilities'
import type { AcpPermissionPolicy, AcpRuntimeProfile } from '@/types/api'

export interface AcpRuntimeCapabilityFields {
  profile: AcpRuntimeProfile
  command: string
  argsText: string
  envText: string
  permissionPolicy: AcpPermissionPolicy
  model: string
}

function toInput(fields: AcpRuntimeCapabilityFields): AcpRuntimeCapabilitiesInput {
  return {
    profile: fields.profile,
    command: fields.command,
    args: parseAcpArgs(fields.argsText),
    env: parseAcpEnv(fields.envText),
    permission_policy: fields.permissionPolicy,
    selected_model: fields.model.trim() || null,
  }
}

function inputsEqual(
  left: AcpRuntimeCapabilitiesInput | null,
  right: AcpRuntimeCapabilitiesInput,
) {
  return left !== null && JSON.stringify(left) === JSON.stringify(right)
}

export function useCommittedAcpRuntimeCapabilities(
  fields: AcpRuntimeCapabilityFields,
  enabled: boolean,
) {
  const currentInput = useMemo(
    () =>
      toInput({
        profile: fields.profile,
        command: fields.command,
        argsText: fields.argsText,
        envText: fields.envText,
        permissionPolicy: fields.permissionPolicy,
        model: fields.model,
      }),
    [
      fields.argsText,
      fields.command,
      fields.envText,
      fields.model,
      fields.permissionPolicy,
      fields.profile,
    ],
  )
  const [committedInput, setCommittedInput] =
    useState<AcpRuntimeCapabilitiesInput | null>(null)
  const [stale, setStale] = useState(false)
  const wasEnabled = useRef(false)
  const queryClient = useQueryClient()
  const query = useAcpRuntimeCapabilities(committedInput, enabled && committedInput !== null)
  const refetch = query.refetch

  useEffect(() => {
    if (!enabled) {
      setCommittedInput(null)
      setStale(false)
      wasEnabled.current = false
      return
    }
    if (!wasEnabled.current && currentInput.command.trim()) {
      setCommittedInput(currentInput)
      setStale(false)
    }
    wasEnabled.current = true
  }, [currentInput, enabled])

  const commit = useCallback(
    (overrides: Partial<AcpRuntimeCapabilityFields> = {}) => {
      setCommittedInput(toInput({ ...fields, ...overrides }))
      setStale(false)
    },
    [fields],
  )

  const markStale = useCallback(() => setStale(true), [])

  const commitProfile = useCallback(
    (profile: AcpRuntimeProfile) => {
      setCommittedInput((previous) =>
        previous ? { ...previous, profile } : toInput({ ...fields, profile }),
      )
    },
    [fields],
  )

  const commitModel = useCallback(
    (model: string) => {
      setCommittedInput((previous) =>
        previous
          ? { ...previous, selected_model: model.trim() || null }
          : toInput({ ...fields, model }),
      )
    },
    [fields],
  )

  const refresh = useCallback(() => {
    const nextInput = toInput(fields)
    setStale(false)
    if (inputsEqual(committedInput, nextInput)) {
      void refetch()
      return
    }
    void queryClient.invalidateQueries({
      queryKey: acpRuntimeCapabilitiesQueryKey(nextInput),
      exact: true,
      refetchType: 'none',
    })
    setCommittedInput(nextInput)
  }, [committedInput, fields, queryClient, refetch])

  return {
    ...query,
    capabilitiesStale: stale,
    commit,
    commitProfile,
    commitModel,
    markStale,
    refresh,
  }
}
