import { useEffect, useId, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { useRenameDirectChat } from '@/hooks/useDirectChats'

interface Props { chatId: string; title: string }

export function EditableDirectChatTitle({ chatId, title }: Props) {
  const { t } = useTranslation('chat')
  const rename = useRenameDirectChat(chatId)
  const [editing, setEditing] = useState(false)
  const [value, setValue] = useState(title)
  const [error, setError] = useState<string | null>(null)
  const savingRef = useRef(false)
  const errorId = useId()
  useEffect(() => { if (!editing) setValue(title) }, [editing, title])
  const save = async () => {
    if (savingRef.current) return
    const trimmed = value.trim()
    if (!trimmed || Array.from(trimmed).length > 120) { setError(t('direct.titleInvalid')); return }
    savingRef.current = true
    try { setError(null); await rename.mutateAsync({ title: trimmed }); setEditing(false) }
    catch (cause) { setError(cause instanceof Error ? cause.message : String(cause)) }
    finally { savingRef.current = false }
  }
  if (editing) return <span className="inline-flex min-w-0 flex-col"><input autoFocus value={value} onChange={(e) => setValue(e.target.value)} onBlur={() => void save()} onKeyDown={(e) => { if (e.key === 'Enter') void save(); if (e.key === 'Escape') { setEditing(false); setError(null) } }} aria-label={t('direct.rename')} aria-describedby={error ? errorId : undefined} className="h-7 max-w-72 rounded border border-input bg-background px-2 text-base font-semibold" />{error ? <span id={errorId} role="alert" className="text-xs text-destructive">{error}</span> : null}</span>
  return <button type="button" className="truncate text-left" onClick={() => { savingRef.current = false; setEditing(true) }} aria-label={t('direct.rename')}>{title}</button>
}
