import { Sparkles, WandSparkles } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useGenerateAgentSystemPrompt } from '@/hooks/useGenerateAgentSystemPrompt'

interface AgentSystemPromptActionsProps {
  name: string
  description?: string
  prompt: string
  providerId?: string
  model?: string
  onApply: (prompt: string) => void
}

export function AgentSystemPromptActions({
  name,
  description,
  prompt,
  providerId,
  model,
  onApply,
}: AgentSystemPromptActionsProps) {
  const { t } = useTranslation('agents')
  const generate = useGenerateAgentSystemPrompt()
  const [error, setError] = useState<string | null>(null)

  const run = async (enhance: boolean) => {
    if (!providerId) return
    setError(null)
    try {
      const result = await generate.mutateAsync({
        name: name.trim() || undefined,
        description: description?.trim() || null,
        system_prompt: enhance ? prompt : null,
        llm_provider_id: providerId,
        model: model?.trim() || null,
      })
      onApply(result.system_prompt)
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason))
    }
  }

  return (
    <div className="flex flex-wrap items-center justify-end gap-1.5">
      {!providerId ? (
        <span className="text-2xs text-muted-foreground">{t('promptAi.providerRequired')}</span>
      ) : null}
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 px-2 text-xs"
        disabled={!providerId || generate.isPending}
        onClick={() => void run(false)}
      >
        <WandSparkles className="h-3.5 w-3.5" aria-hidden />
        {t('promptAi.generate')}
      </Button>
      <Button
        type="button"
        variant="outline"
        size="sm"
        className="h-7 gap-1.5 px-2 text-xs"
        disabled={!providerId || !prompt.trim() || generate.isPending}
        onClick={() => void run(true)}
      >
        <Sparkles className="h-3.5 w-3.5" aria-hidden />
        {generate.isPending ? t('promptAi.working') : t('promptAi.enhance')}
      </Button>
      {error ? <span role="alert" className="w-full text-right text-2xs text-destructive">{error}</span> : null}
    </div>
  )
}
