import { useEffect, useRef, useState, type ChangeEvent } from 'react'
import { Check, ImagePlus, Loader2, MessagesSquare, Pencil } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { AgentAvatar } from '@/components/chat/AgentAvatar'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from '@/components/ui/dialog'
import { useUpdateGroup } from '@/hooks/useGroups'
import {
  AGENT_AVATAR_ACCEPT,
  resizeAgentAvatar,
  validateAgentAvatarFile,
} from '@/lib/agentAvatar'
import { ApiError } from '@/lib/api-v2/client'
import { cn } from '@/lib/utils'

export interface GroupAvatarMember {
  id: string
  name: string
  kind: 'user' | 'agent'
  avatarUrl?: string | null
  avatar_url?: string | null
}

interface GroupAvatarProps {
  name: string
  avatarUrl?: string | null
  members: GroupAvatarMember[]
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

/** Uploaded image when present; otherwise members orbit the group mark in a live ring. */
export function GroupAvatar({
  name,
  avatarUrl,
  members,
  size = 'md',
  className,
}: GroupAvatarProps) {
  const custom = avatarUrl?.startsWith('data:image/') ? avatarUrl : null
  const visible = members.slice(0, 6)
  const frameSize = {
    sm: 'h-7 w-7',
    md: 'h-12 w-12',
    lg: 'h-16 w-16',
  }[size]
  const singleSize = { sm: 'sm', md: 'md', lg: 'lg' }[size] as 'sm' | 'md' | 'lg'
  const orbitTileSize = size === 'lg' ? 'sm' : 'xs'
  const orbitRadius = { sm: 8, md: 16, lg: 20 }[size]

  return (
    <span
      aria-hidden="true"
      data-slot="group-avatar"
      data-member-count={visible.length}
      data-custom={custom ? 'true' : undefined}
      className={cn(
        'relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full bg-muted/60 shadow-xs ring-1 ring-inset ring-border/80',
        frameSize,
        className,
      )}
    >
      {custom ? (
        <img src={custom} alt="" className="h-full w-full object-cover" />
      ) : visible.length === 0 ? (
        <span className="flex h-full w-full items-center justify-center bg-primary/12 text-primary">
          <MessagesSquare className={size === 'lg' ? 'h-7 w-7' : 'h-5 w-5'} />
        </span>
      ) : visible.length === 1 ? (
        <AgentAvatar
          name={visible[0]!.name || name}
          kind={visible[0]!.kind}
          avatarUrl={visible[0]!.avatarUrl ?? visible[0]!.avatar_url}
          size={singleSize}
        />
      ) : (
        <>
          <span className="absolute inset-[21%] rounded-full border border-primary/20 bg-primary/[0.04]" />
          <span
            className={cn(
              'absolute left-1/2 top-1/2 z-10 flex -translate-x-1/2 -translate-y-1/2 items-center justify-center rounded-full bg-background text-primary shadow-xs ring-1 ring-primary/20',
              size === 'sm' ? 'h-2 w-2' : size === 'md' ? 'h-4 w-4' : 'h-5 w-5',
            )}
          >
            {size === 'sm' ? (
              <span className="h-1 w-1 rounded-full bg-primary" />
            ) : (
              <MessagesSquare className={size === 'lg' ? 'h-3 w-3' : 'h-2.5 w-2.5'} />
            )}
          </span>
          {visible.map((member, index) => {
            const offset = visible.length === 2 ? -45 : 0
            const angle = offset + (index * 360) / visible.length
            return (
              <span
                key={member.id}
                className="absolute left-1/2 top-1/2 z-20 inline-flex"
                style={{
                  transform: `translate(-50%, -50%) rotate(${angle}deg) translateY(-${orbitRadius}px) rotate(${-angle}deg)`,
                }}
              >
                <AgentAvatar
                  name={member.name}
                  kind={member.kind}
                  avatarUrl={member.avatarUrl ?? member.avatar_url}
                  size={orbitTileSize}
                  className={cn(
                    'rounded-full shadow-xs ring-1 ring-background',
                    size === 'sm' && 'scale-75',
                  )}
                />
              </span>
            )
          })}
        </>
      )}
    </span>
  )
}

interface GroupAvatarEditorProps extends GroupAvatarProps {
  groupId: string
}

export function GroupAvatarEditor({
  groupId,
  name,
  avatarUrl,
  members,
}: GroupAvatarEditorProps) {
  const { t } = useTranslation(['groups', 'common'])
  const inputRef = useRef<HTMLInputElement>(null)
  const update = useUpdateGroup(groupId)
  const [open, setOpen] = useState(false)
  const [displayAvatar, setDisplayAvatar] = useState(avatarUrl ?? null)
  const [processing, setProcessing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const custom = displayAvatar?.startsWith('data:image/') ? displayAvatar : null
  const locked = processing || update.isPending

  useEffect(() => {
    setDisplayAvatar(avatarUrl ?? null)
  }, [avatarUrl])

  const saveAvatar = async (next: string | null) => {
    const previous = displayAvatar
    setDisplayAvatar(next)
    setError(null)
    try {
      await update.mutateAsync({ avatar_url: next })
    } catch (cause) {
      setDisplayAvatar(previous)
      setError(
        cause instanceof ApiError
          ? t('groups:errors.updateDetail', { message: cause.message })
          : t('groups:errors.update'),
      )
    }
  }

  const upload = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0]
    event.target.value = ''
    if (!file) return
    const validation = validateAgentAvatarFile(file)
    if (validation) {
      setError(t(`groups:manage.avatar.${validation}`))
      return
    }
    setProcessing(true)
    setError(null)
    try {
      await saveAvatar(await resizeAgentAvatar(file))
    } catch {
      setError(t('groups:manage.avatar.processingFailed'))
    } finally {
      setProcessing(false)
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        setOpen(next)
        if (next) setError(null)
      }}
    >
      <DialogTrigger asChild>
        <button
          type="button"
          className="group relative shrink-0 rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-card"
          aria-label={t('groups:manage.avatar.edit')}
          title={t('groups:manage.avatar.edit')}
        >
          <GroupAvatar name={name} avatarUrl={displayAvatar} members={members} />
          <span className="absolute -bottom-1 -right-1 flex h-5 w-5 items-center justify-center rounded-full border-2 border-card bg-primary text-primary-foreground shadow-sm transition-transform group-hover:scale-105">
            <Pencil className="h-2.5 w-2.5" />
          </span>
        </button>
      </DialogTrigger>

      <DialogContent closeLabel={t('common:actions.close')} className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>{t('groups:manage.avatar.title')}</DialogTitle>
          <DialogDescription>{t('groups:manage.avatar.description')}</DialogDescription>
        </DialogHeader>

        <div className="grid grid-cols-2 gap-3">
          <button
            type="button"
            disabled={locked}
            aria-pressed={!displayAvatar}
            onClick={() => {
              if (displayAvatar) void saveAvatar(null)
            }}
            className={cn(
              'relative flex min-h-40 flex-col items-center justify-center gap-3 rounded-lg border bg-card p-4 text-center outline-none transition-colors',
              'hover:bg-card-hover focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-60',
              !displayAvatar ? 'border-primary ring-1 ring-primary' : 'border-border',
            )}
          >
            {!displayAvatar ? (
              <span className="absolute right-2.5 top-2.5 flex h-5 w-5 items-center justify-center rounded-full bg-primary text-primary-foreground">
                <Check className="h-3 w-3" />
              </span>
            ) : null}
            <GroupAvatar name={name} members={members} size="lg" />
            <span>
              <span className="block text-sm font-semibold">{t('groups:manage.avatar.composite')}</span>
              <span className="mt-1 block text-xs leading-4 text-muted-foreground">
                {t('groups:manage.avatar.compositeDescription')}
              </span>
            </span>
          </button>

          <button
            type="button"
            disabled={locked}
            aria-pressed={Boolean(custom)}
            onClick={() => inputRef.current?.click()}
            className={cn(
              'relative flex min-h-40 flex-col items-center justify-center gap-3 rounded-lg border bg-card p-4 text-center outline-none transition-colors',
              'hover:bg-card-hover focus-visible:ring-2 focus-visible:ring-ring disabled:pointer-events-none disabled:opacity-60',
              custom ? 'border-primary ring-1 ring-primary' : 'border-border border-dashed',
            )}
          >
            {custom ? (
              <span className="absolute right-2.5 top-2.5 flex h-5 w-5 items-center justify-center rounded-full bg-primary text-primary-foreground">
                <Check className="h-3 w-3" />
              </span>
            ) : null}
            {processing ? (
              <span className="flex h-16 w-16 items-center justify-center rounded-2xl bg-muted text-muted-foreground">
                <Loader2 className="h-5 w-5 animate-spin" />
              </span>
            ) : custom ? (
              <GroupAvatar name={name} avatarUrl={custom} members={members} size="lg" />
            ) : (
              <span className="flex h-16 w-16 items-center justify-center rounded-2xl border border-dashed border-border bg-muted/50 text-muted-foreground">
                <ImagePlus className="h-6 w-6" />
              </span>
            )}
            <span>
              <span className="block text-sm font-semibold">{t('groups:manage.avatar.upload')}</span>
              <span className="mt-1 block text-xs leading-4 text-muted-foreground">
                {t('groups:manage.avatar.uploadDescription')}
              </span>
            </span>
          </button>
          <input
            ref={inputRef}
            type="file"
            accept={AGENT_AVATAR_ACCEPT}
            disabled={locked}
            className="sr-only"
            aria-label={t('groups:manage.avatar.upload')}
            onChange={upload}
          />
        </div>

        {error ? <p role="alert" className="text-xs text-destructive">{error}</p> : null}

        <DialogFooter>
          <DialogClose asChild>
            <Button type="button" variant="outline" disabled={locked}>
              {t('common:actions.close')}
            </Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
