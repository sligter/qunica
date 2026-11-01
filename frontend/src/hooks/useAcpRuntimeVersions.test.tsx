import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { PropsWithChildren } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import {
  useAcpRuntimeVersions,
  useInstallAcpRuntimeVersion,
} from '@/hooks/useAcpRuntimeVersions'
import { useAuthStore } from '@/stores/authStore'

describe('ACP runtime version hooks', () => {
  afterEach(() => {
    useAuthStore.setState({ token: null })
    vi.unstubAllGlobals()
  })

  it.each([
    ['codex', '@agentclientprotocol/codex-acp@next'],
    ['codex-cli', undefined],
  ] as const)('loads versions and refreshes presets after installing %s', async (presetId, packageSpec) => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ presets: [] }), {
          status: 200,
          headers: { 'Content-Type': 'application/json' },
        }),
      )
      .mockResolvedValueOnce(
        new Response(
          JSON.stringify({
            preset: {
              id: presetId,
              package_name: presetId === 'codex-cli' ? '@openai/codex' : '@agentclientprotocol/codex-acp',
              installed: true,
              local_version: '1.0.0',
              latest_version: '1.0.0',
              status: 'current',
              message: null,
            },
            output: 'installed',
          }),
          { status: 200, headers: { 'Content-Type': 'application/json' } },
        ),
      )
      // Installation invalidates the active query and awaits this refetch.
      .mockResolvedValueOnce(new Response(JSON.stringify({ presets: [] }), {
        status: 200, headers: { 'Content-Type': 'application/json' },
      }))
    vi.stubGlobal('fetch', fetchMock)
    useAuthStore.setState({ token: 'owner-token' })
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    })
    const invalidateQueries = vi.spyOn(queryClient, 'invalidateQueries')
    const wrapper = ({ children }: PropsWithChildren) => (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    )

    const { result } = renderHook(
      () => ({ versions: useAcpRuntimeVersions(), install: useInstallAcpRuntimeVersion() }),
      { wrapper },
    )

    await waitFor(() => expect(result.current.versions.isSuccess).toBe(true))
    await result.current.install.mutateAsync({
      presetId,
      packageSpec,
    })

    const [url, init] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(url).toContain(`/agents/acp-runtime-versions/${presetId}/install`)
    expect(JSON.parse(String(init.body))).toEqual(packageSpec ? { package_spec: packageSpec } : {})
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['agents', 'acp-runtime-versions'],
    })
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['agents', 'acp-runtime-presets'],
    })
  })
})
