import { Sparkles } from 'lucide-react'
import { useTranslation } from 'react-i18next'

export function SkillsIndexPage() {
  const { t } = useTranslation('skills')
  return (
    <div className="flex h-full w-full flex-col items-center justify-center gap-3 bg-background p-6 text-center">
      <div className="flex h-14 w-14 items-center justify-center rounded-full bg-muted text-muted-foreground">
        <Sparkles className="h-7 w-7" />
      </div>
      <h2 className="text-base font-medium">{t('list.selectTitle')}</h2>
      <p className="max-w-sm text-sm text-muted-foreground">
        {t('list.selectDescription')}
      </p>
    </div>
  )
}
