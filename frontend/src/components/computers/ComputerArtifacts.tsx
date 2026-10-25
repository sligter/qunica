import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Button } from '@/components/ui/button'
import { fetchJson } from '@/lib/api-v2/client'
import { authFetch } from '@/lib/authFetch'
import { apiUrl } from '@/lib/runtime'
import { useAuthStore } from '@/stores/authStore'

// Keep in sync with the computer broker and backend transfer limit.
const MAX_ARTIFACT_BYTES = 1000 * 1024 * 1024

export function ComputerArtifacts({ id }: { id: string }) {
  const { t } = useTranslation('computers')
  const token = useAuthStore(s => s.token)
  const [open, setOpen] = useState(false)
  const [error, setError] = useState('')
  const files = useQuery({ queryKey: ['computer-artifacts', id], queryFn: () => fetchJson<{ items: { id: string; path: string; size: number; turn_id: string }[] }>(`/computers/${id}/artifacts`, { token }), enabled: open && !!token, refetchInterval: open ? 10_000 : false })
  const download = async (id: string, path: string) => {
    try {
      const response = await authFetch(apiUrl(`/api/v2/computer-artifacts/${id}`), { headers: { Authorization: `Bearer ${token}` } })
      if (!response.ok) throw new Error(t('artifactUnavailable'))
      const url = URL.createObjectURL(await response.blob())
      const link = document.createElement('a'); link.href = url; link.download = path.split('/').at(-1) ?? 'artifact'; link.click()
      setTimeout(() => URL.revokeObjectURL(url), 1000)
      setError('')
    } catch (error) { setError(error instanceof Error ? error.message : t('artifactUnavailable')) }
  }
  return <div className="mt-3 border-t border-border pt-3">
    <Button size="sm" variant="ghost" onClick={() => setOpen(!open)} aria-expanded={open}>{t('artifacts')}</Button>
    {open ? <ul className="mt-2 space-y-2 text-xs">{files.data?.items?.map(file => <li key={file.id} className="flex items-center justify-between gap-3"><div className="min-w-0"><p className="truncate font-mono" title={file.path}>{file.path}</p><p className="mt-0.5 truncate text-2xs text-muted-foreground" title={file.turn_id}>{file.size.toLocaleString()} B · {file.turn_id}</p></div><Button size="sm" variant="outline" disabled={file.size > MAX_ARTIFACT_BYTES} onClick={() => void download(file.id, file.path)}>{t('download')}</Button></li>)}</ul> : null}
    {open && files.data?.items?.length === 0 ? <p className="mt-2 text-xs text-muted-foreground">{t('noArtifacts')}</p> : null}
    {error || files.isError ? <p role="alert" className="mt-2 text-xs text-destructive">{error || files.error?.message}</p> : null}
  </div>
}
