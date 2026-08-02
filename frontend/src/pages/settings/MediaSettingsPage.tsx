import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { DetailShell } from '@/components/layout/DetailShell'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { SettingsRow, SettingsSection } from '@/components/ui/settings-row'
import { useSystemSettings, useUpdateSystemSettings } from '@/hooks/useSystemSettings'
import { ApiError } from '@/lib/api-v2/client'
import type { SystemSettingsUpdate } from '@/types/api'

interface MediaDraft {
  baseUrl: string
  apiKey: string
  imageModel: string
  imageEndpoint: string
  videoModel: string
  videoEndpoint: string
  videoStatusEndpoint: string
  videoContentEndpoint: string
}

const EMPTY_DRAFT: MediaDraft = {
  baseUrl: 'https://api.openai.com',
  apiKey: '',
  imageModel: '',
  imageEndpoint: '/v1/images/generations',
  videoModel: '',
  videoEndpoint: '/v1/videos',
  videoStatusEndpoint: '/v1/videos/{id}',
  videoContentEndpoint: '/v1/videos/{id}/content',
}

export function MediaSettingsPage() {
  const { t, i18n } = useTranslation('settings')
  const settings = useSystemSettings()
  const update = useUpdateSystemSettings()
  const [draft, setDraft] = useState<MediaDraft>(EMPTY_DRAFT)
  const [clearKey, setClearKey] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!settings.data) return
    setDraft({
      baseUrl: settings.data.media_base_url,
      apiKey: '',
      imageModel: settings.data.image_generation_model ?? '',
      imageEndpoint: settings.data.image_generation_endpoint,
      videoModel: settings.data.video_generation_model ?? '',
      videoEndpoint: settings.data.video_generation_endpoint,
      videoStatusEndpoint: settings.data.video_status_endpoint,
      videoContentEndpoint: settings.data.video_content_endpoint,
    })
    setClearKey(false)
  }, [settings.data])

  useEffect(() => {
    document.title = t('media.documentTitle')
  }, [i18n.resolvedLanguage, t])

  const server = settings.data
  const keyReady = Boolean(
    draft.apiKey.trim() || (server?.media_api_key_configured && !clearKey),
  )
  const imageReady = keyReady && Boolean(draft.imageModel.trim())
  const videoReady = keyReady && Boolean(draft.videoModel.trim())
  const dirty = Boolean(
    server && (
      draft.apiKey.trim() ||
      clearKey ||
      draft.baseUrl.trim() !== server.media_base_url ||
      draft.imageModel.trim() !== (server.image_generation_model ?? '') ||
      draft.imageEndpoint.trim() !== server.image_generation_endpoint ||
      draft.videoModel.trim() !== (server.video_generation_model ?? '') ||
      draft.videoEndpoint.trim() !== server.video_generation_endpoint ||
      draft.videoStatusEndpoint.trim() !== server.video_status_endpoint ||
      draft.videoContentEndpoint.trim() !== server.video_content_endpoint
    )
  )

  const change = (patch: Partial<MediaDraft>) => {
    setDraft((current) => ({ ...current, ...patch }))
  }

  const onSave = async () => {
    setError(null)
    const patch: SystemSettingsUpdate = {
      media_base_url: draft.baseUrl.trim() || null,
      image_generation_model: draft.imageModel.trim() || null,
      image_generation_endpoint: draft.imageEndpoint.trim() || null,
      video_generation_model: draft.videoModel.trim() || null,
      video_generation_endpoint: draft.videoEndpoint.trim() || null,
      video_status_endpoint: draft.videoStatusEndpoint.trim() || null,
      video_content_endpoint: draft.videoContentEndpoint.trim() || null,
    }
    if (clearKey) patch.media_api_key = null
    else if (draft.apiKey.trim()) patch.media_api_key = draft.apiKey.trim()

    try {
      await update.mutateAsync(patch)
    } catch (err) {
      setError(err instanceof ApiError ? err.message : t('errors.network'))
    }
  }

  const statusBadge = (ready: boolean) => (
    <Badge
      variant="outline"
      className={ready ? 'border-primary/40 bg-primary/10 text-primary' : 'text-muted-foreground'}
    >
      {t(ready ? 'media.configured' : 'media.notConfigured')}
    </Badge>
  )

  return (
    <DetailShell
      title={t('media.title')}
      subtitle={t('media.subtitle')}
      contentClassName="max-w-none"
      actions={
        <Button size="sm" onClick={() => void onSave()} disabled={!dirty || update.isPending}>
          {update.isPending ? t('common:actions.saving') : t('common:actions.save')}
        </Button>
      }
    >
      <div className="space-y-10">
        <SettingsSection
          title={t('media.provider.title')}
          description={t('media.provider.description')}
        >
          <SettingsRow
            label={t('media.provider.baseUrl')}
            description={t('media.provider.baseUrlDescription')}
            htmlFor="media-base-url"
            stacked
          >
            <Input
              id="media-base-url"
              value={draft.baseUrl}
              onChange={(event) => change({ baseUrl: event.target.value })}
              placeholder="https://api.openai.com"
              disabled={settings.isLoading || update.isPending}
            />
          </SettingsRow>
          <SettingsRow
            label={t('media.provider.apiKey')}
            description={
              clearKey
                ? t('media.provider.keyWillClear')
                : server?.media_api_key_configured
                  ? t('media.provider.keyConfigured')
                  : t('media.provider.keyMissing')
            }
            htmlFor="media-api-key"
            stacked
          >
            <div className="flex w-full gap-2">
              <Input
                id="media-api-key"
                type="password"
                value={draft.apiKey}
                onChange={(event) => {
                  change({ apiKey: event.target.value })
                  setClearKey(false)
                }}
                placeholder={
                  server?.media_api_key_configured
                    ? t('media.provider.configuredPlaceholder')
                    : 'sk-...'
                }
                disabled={settings.isLoading || update.isPending}
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => {
                  change({ apiKey: '' })
                  setClearKey(true)
                }}
                disabled={!server?.media_api_key_configured || update.isPending}
              >
                {t('media.provider.clearKey')}
              </Button>
            </div>
          </SettingsRow>
        </SettingsSection>

        <SettingsSection
          title={t('media.image.title')}
          description={t('media.image.description')}
          aside={statusBadge(imageReady)}
        >
          <SettingsRow
            label={t('media.model')}
            description={t('media.image.modelDescription')}
            htmlFor="media-image-model"
          >
            <Input
              id="media-image-model"
              value={draft.imageModel}
              onChange={(event) => change({ imageModel: event.target.value })}
              placeholder="gpt-image-1"
              disabled={settings.isLoading || update.isPending}
            />
          </SettingsRow>
          <SettingsRow
            label={t('media.endpoint')}
            description={t('media.endpointDescription')}
            htmlFor="media-image-endpoint"
            stacked
          >
            <Input
              id="media-image-endpoint"
              value={draft.imageEndpoint}
              onChange={(event) => change({ imageEndpoint: event.target.value })}
              placeholder="/v1/images/generations"
              disabled={settings.isLoading || update.isPending}
            />
          </SettingsRow>
        </SettingsSection>

        <SettingsSection
          title={t('media.video.title')}
          description={t('media.video.description')}
          aside={statusBadge(videoReady)}
        >
          <SettingsRow
            label={t('media.model')}
            description={t('media.video.modelDescription')}
            htmlFor="media-video-model"
          >
            <Input
              id="media-video-model"
              value={draft.videoModel}
              onChange={(event) => change({ videoModel: event.target.value })}
              placeholder="sora-2"
              disabled={settings.isLoading || update.isPending}
            />
          </SettingsRow>
          <SettingsRow
            label={t('media.video.createEndpoint')}
            description={t('media.endpointDescription')}
            htmlFor="media-video-endpoint"
            stacked
          >
            <Input
              id="media-video-endpoint"
              value={draft.videoEndpoint}
              onChange={(event) => change({ videoEndpoint: event.target.value })}
              placeholder="/v1/videos"
              disabled={settings.isLoading || update.isPending}
            />
          </SettingsRow>
          <SettingsRow
            label={t('media.video.statusEndpoint')}
            description={t('media.video.templateDescription')}
            htmlFor="media-video-status-endpoint"
            stacked
          >
            <Input
              id="media-video-status-endpoint"
              value={draft.videoStatusEndpoint}
              onChange={(event) => change({ videoStatusEndpoint: event.target.value })}
              placeholder="/v1/videos/{id}"
              disabled={settings.isLoading || update.isPending}
            />
          </SettingsRow>
          <SettingsRow
            label={t('media.video.contentEndpoint')}
            description={t('media.video.templateDescription')}
            htmlFor="media-video-content-endpoint"
            stacked
          >
            <Input
              id="media-video-content-endpoint"
              value={draft.videoContentEndpoint}
              onChange={(event) => change({ videoContentEndpoint: event.target.value })}
              placeholder="/v1/videos/{id}/content"
              disabled={settings.isLoading || update.isPending}
            />
          </SettingsRow>
        </SettingsSection>

        <p className="text-xs text-muted-foreground">{t('media.outputHint')}</p>
        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {error}
          </p>
        ) : null}
      </div>
    </DetailShell>
  )
}
