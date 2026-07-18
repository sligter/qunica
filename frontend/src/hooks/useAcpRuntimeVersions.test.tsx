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

  it('loads runtime versions and refreshes presets after installation', async () => {
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
              id: 'codex',
              package_name: '@agentclientprotocol/codex-acp',
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
      presetId: 'codex',
      packageSpec: '@agentclientprotocol/codex-acp@next',
    })

    const [url, init] = fetchMock.mock.calls[1] as [string, RequestInit]
    expect(url).toContain('/agents/acp-runtime-versions/codex/install')
    expect(JSON.parse(String(init.body))).toEqual({
      package_spec: '@agentclientprotocol/codex-acp@next',
    })
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['agents', 'acp-runtime-versions'],
    })
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ['agents', 'acp-runtime-presets'],
    })
  })
})
