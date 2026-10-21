import { useState } from 'react'
import { ArrowLeft, BookOpen } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import ReactMarkdown from 'react-markdown'

import { Button } from '@/components/ui/button'
import {
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from '@/components/ui/dialog'
import computersGuide from '../../../../backend-rs/crates/backend/src/docs/guide/computers.md?raw'

// Note: bundle the canonical guide so setup remains readable offline and before
// its source revision is published. Commands and details share that source.
const DEPLOYMENT_GUIDE = computersGuide.replace(/\r\n/g, '\n').split('## Deployment\n')[1]?.split('\n## ')[0].trim() ?? computersGuide
const SETUP_COMMANDS = DEPLOYMENT_GUIDE.match(/```sh\n([\s\S]*?)\n```/)?.[1] ?? ''

export function ComputerSetupGuide({ open, onOpenChange }: {
  open: boolean
  onOpenChange: (open: boolean) => void
}) {
  const { t } = useTranslation('computers')
  const [showDocumentation, setShowDocumentation] = useState(false)

  return <Dialog open={open} onOpenChange={(next) => { onOpenChange(next); if (!next) setShowDocumentation(false) }}>
    <DialogContent className="max-h-[85dvh] w-[calc(100%-2rem)] max-w-2xl overflow-y-auto" closeLabel={t('common:actions.close')}>
      <DialogHeader className="pr-6">
        <DialogTitle>{t('setup.title')}</DialogTitle>
        <DialogDescription>{t('setup.description')}</DialogDescription>
      </DialogHeader>
      {showDocumentation ? <>
        <Button variant="ghost" size="sm" className="w-fit" onClick={() => setShowDocumentation(false)}><ArrowLeft className="h-3.5 w-3.5" aria-hidden="true" />{t('setup.backToSteps')}</Button>
        <article aria-label={t('setup.documentation')} className="min-w-0 space-y-4 text-sm leading-6 [&_code]:font-mono [&_code]:text-xs [&_pre]:whitespace-pre-wrap [&_pre]:break-words [&_pre]:rounded-md [&_pre]:border [&_pre]:border-border [&_pre]:bg-muted/40 [&_pre]:p-3 [&_pre]:[overflow-wrap:anywhere]">
          <ReactMarkdown>{DEPLOYMENT_GUIDE}</ReactMarkdown>
        </article>
      </> : <>
      <ol className="min-w-0 space-y-5 text-sm leading-6">
        <li>
          <h3 className="font-medium">{t('setup.prepareTitle')}</h3>
          <p className="mt-1 text-muted-foreground">{t('setup.prepare')}</p>
        </li>
        <li className="min-w-0">
          <h3 className="font-medium">{t('setup.deployTitle')}</h3>
          <p className="mt-1 text-muted-foreground">{t('setup.deploy')}</p>
          <pre className="mt-3 max-w-full whitespace-pre-wrap break-words rounded-md border border-border bg-muted/40 p-3 font-mono text-xs leading-6 [overflow-wrap:anywhere]"><code>{SETUP_COMMANDS}</code></pre>
        </li>
        <li>
          <h3 className="font-medium">{t('setup.connectTitle')}</h3>
          <p className="mt-1 text-muted-foreground">{t('setup.connect')}</p>
        </li>
      </ol>
      <div className="rounded-md border border-border bg-muted/40 p-3 text-xs leading-6 text-muted-foreground">
        <p className="font-medium text-foreground">{t('setup.instancesTitle')}</p>
        <p>{t('setup.instances')}</p>
        <p className="mt-2">{t('setup.existing')}</p>
      </div>
      <Button variant="link" size="sm" className="h-auto w-fit px-0" onClick={() => setShowDocumentation(true)}><BookOpen className="h-3.5 w-3.5" aria-hidden="true" />{t('setup.documentation')}</Button>
      </>}
    </DialogContent>
  </Dialog>
}
