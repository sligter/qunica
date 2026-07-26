import { useEffect, useState } from 'react'
import { FileText, Image as ImageIcon, Paperclip } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { ImageLightbox } from '@/components/chat/ImageLightbox'
import { downloadGroupWorkspaceFile } from '@/hooks/useGroupFiles'
import { apiUrl } from '@/lib/runtime'
import { useAuthStore } from '@/stores/authStore'
import type { MessageAttachment } from '@/types/api'

function formatSize(size: number) {
  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

function ImagePreview({ groupId, attachment }: { groupId: string; attachment: MessageAttachment }) {
  const token = useAuthStore((state) => state.token)
  const [url, setUrl] = useState<string | null>(null)
  const [failed, setFailed] = useState(false)
  const [previewOpen, setPreviewOpen] = useState(false)
  useEffect(() => {
    let active = true
    let objectUrl: string | null = null
    const headers = token ? { Authorization: `Bearer ${token}` } : undefined
    void fetch(apiUrl(`/api/v2/groups/${groupId}/workspace-files/download?path=${encodeURIComponent(attachment.path)}`), { headers })
      .then((response) => response.ok ? response.blob() : Promise.reject(new Error('preview failed')))
      .then((blob) => {
        objectUrl = URL.createObjectURL(blob)
        if (active) setUrl(objectUrl)
        else URL.revokeObjectURL(objectUrl)
      })
      .catch(() => { if (active) setFailed(true) })
    return () => { active = false; if (objectUrl) URL.revokeObjectURL(objectUrl) }
  }, [attachment.path, groupId, token])
  if (failed || !url) return null
  return <>
    <button
      type="button"
      className="block max-w-full rounded focus:outline-none focus:ring-2 focus:ring-ring"
      onClick={() => setPreviewOpen(true)}
      aria-label={`Preview ${attachment.name}`}
    >
      <img src={url} alt={attachment.name} className="max-h-52 max-w-full rounded object-contain" onError={() => setFailed(true)} />
    </button>
    <ImageLightbox open={previewOpen} onOpenChange={setPreviewOpen} src={url} alt={attachment.name} />
  </>
}

export function MessageAttachments({ groupId, attachments }: { groupId: string; attachments: MessageAttachment[] }) {
  const { t } = useTranslation('chat')
  const token = useAuthStore((state) => state.token)
  return <div className="mt-2 space-y-2">
    {attachments.map((attachment) => <div key={attachment.id} className="min-w-0 rounded-md border border-border/70 p-2">
      {attachment.kind === 'image' ? <ImagePreview groupId={groupId} attachment={attachment} /> : null}
      <div className="mt-1 flex min-w-0 items-center gap-2">
        {attachment.kind === 'image' ? <ImageIcon className="h-4 w-4 shrink-0" /> : <FileText className="h-4 w-4 shrink-0" />}
        <div className="min-w-0 flex-1"><div className="truncate text-xs font-medium">{attachment.name}</div><div className="flex flex-wrap gap-x-1 text-2xs text-muted-foreground"><span>{attachment.mime_type}</span><span aria-hidden="true">·</span><span>{formatSize(attachment.size)}</span></div></div>
        <Button type="button" variant="ghost" size="icon" className="h-7 w-7" aria-label={t('attachments.openNamed', { name: attachment.name, defaultValue: `Open ${attachment.name}` })} title={t('attachments.open', { defaultValue: 'Open attachment' })} onClick={() => void downloadGroupWorkspaceFile(groupId, attachment.path, token)}><Paperclip className="h-3.5 w-3.5" /></Button>
      </div>
    </div>)}
  </div>
}
