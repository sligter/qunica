import { useRef, useState } from 'react'
import { FileArchive, FolderOpen, Github, Upload } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { FieldError } from '@/components/ui/field'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
import { cn } from '@/lib/utils'
import {
  useImportSkill,
  useImportSkillFromGithub,
  useImportSkillPackage,
} from '@/hooks/useSkills'
import { ApiError } from '@/lib/api-v2/client'
import {
  localizedErrorText,
  messageError,
  translatedError,
  type LocalizedError,
} from '@/i18n/localizedError'

/** Ties the visible "choose file" label to the visually hidden input. */
const ZIP_INPUT_ID = 'skill-package-file'

/** One rendered shape for every tab's failure, instead of three copies. */
function FormError({ message }: { message: string | null | undefined }) {
  if (!message) return null
  return <FieldError>{message}</FieldError>
}

interface ImportSkillFormProps {
  onCreated?: (newSkillId: string) => void
}

export function ImportSkillForm({ onCreated }: ImportSkillFormProps = {}) {
  const { t } = useTranslation('skills')
  const importSkill = useImportSkill()
  const importPackage = useImportSkillPackage()
  const importGithub = useImportSkillFromGithub()
  const [raw, setRaw] = useState('')
  const [error, setError] = useState<LocalizedError | null>(null)
  const [selectedFile, setSelectedFile] = useState<File | null>(null)
  const [githubUrl, setGithubUrl] = useState('')
  const [githubBranch, setGithubBranch] = useState('main')
  const [githubPath, setGithubPath] = useState('')
  const fileInputRef = useRef<HTMLInputElement>(null)

  const onSubmitMd = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    if (!raw.trim()) {
      setError(translatedError('errors.markdownRequired'))
      return
    }
    try {
      const created = await importSkill.mutateAsync({ raw })
      setRaw('')
      onCreated?.(created.id)
    } catch (err) {
      setError(err instanceof ApiError ? messageError(err.message) : translatedError('errors.network'))
    }
  }

  const onSubmitZip = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    if (!selectedFile) {
      setError(translatedError('errors.packageRequired'))
      return
    }
    try {
      const created = await importPackage.mutateAsync(selectedFile)
      setSelectedFile(null)
      if (fileInputRef.current) fileInputRef.current.value = ''
      onCreated?.(created.id)
    } catch (err) {
      setError(err instanceof ApiError ? messageError(err.message) : translatedError('errors.network'))
    }
  }

  const onSubmitGithub = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    if (!githubUrl.trim()) {
      setError(translatedError('errors.githubRequired'))
      return
    }
    try {
      const created = await importGithub.mutateAsync({
        url: githubUrl.trim(),
        branch: githubBranch.trim() || null,
        path: githubPath.trim() || null,
      })
      setGithubUrl('')
      setGithubBranch('main')
      setGithubPath('')
      onCreated?.(created.id)
    } catch (err) {
      setError(err instanceof ApiError ? messageError(err.message) : translatedError('errors.network'))
    }
  }

  const acceptFile = (file: File | undefined) => {
    if (!file) return
    if (file.name.endsWith('.zip')) {
      setSelectedFile(file)
      setError(null)
    } else {
      setError(translatedError('errors.zipOnly'))
    }
  }

  // Tracked so the drop zone can answer the drag. Without this the only cue
  // that a file is over it is the browser's own cursor, and the zone looks
  // identical whether or not it will accept the drop.
  const [dragActive, setDragActive] = useState(false)

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    setDragActive(false)
    acceptFile(e.dataTransfer.files[0])
  }

  const isPending = importSkill.isPending || importPackage.isPending || importGithub.isPending
  const visibleError = localizedErrorText(error, t)

  return (
    <Tabs defaultValue="package" className="space-y-4">
      <TabsList className="w-full">
        <TabsTrigger value="package" className="flex-1">
          <FileArchive className="mr-1.5 h-3.5 w-3.5" />
          {t('form.packageTab')}
        </TabsTrigger>
        <TabsTrigger value="markdown" className="flex-1">
          {t('form.markdownTab')}
        </TabsTrigger>
        <TabsTrigger value="github" className="flex-1">
          <Github className="mr-1.5 h-3.5 w-3.5" />
          {t('form.githubTab')}
        </TabsTrigger>
      </TabsList>

      <TabsContent value="package">
        <form onSubmit={onSubmitZip} className="space-y-4">
          <div
            className={cn(
              'flex flex-col items-center justify-center gap-3 rounded-lg border-2 border-dashed p-8 text-center transition-colors',
              dragActive
                ? 'border-primary bg-primary/5'
                : 'border-border hover:border-primary/40',
            )}
            onDragEnter={(e) => {
              e.preventDefault()
              setDragActive(true)
            }}
            onDragOver={(e) => e.preventDefault()}
            onDragLeave={() => setDragActive(false)}
            onDrop={handleDrop}
          >
            <Upload
              aria-hidden
              className={cn('h-8 w-8', dragActive ? 'text-primary' : 'text-muted-foreground')}
            />
            <div className="min-w-0">
              <p className="truncate text-sm font-medium">
                {selectedFile ? selectedFile.name : t('form.dropPackage')}
              </p>
              {/* Only when a file is chosen: the prompt above is the hint when
                  nothing is, and the button below names the action. */}
              {selectedFile ? (
                <p className="mt-0.5 text-xs text-muted-foreground">
                  {`${(selectedFile.size / 1024).toFixed(1)} KB`}
                </p>
              ) : null}
            </div>
            {/*
              The real control is `sr-only`, not `opacity-0`: a visually hidden
              input still takes focus, so Tab reaches it and Enter opens the
              picker. The old one was transparent but laid out inside the flex
              column, and the button it impersonated did not exist — the only
              way in was to guess where to click.
            */}
            <input
              ref={fileInputRef}
              id={ZIP_INPUT_ID}
              type="file"
              accept=".zip"
              aria-label={t('form.chooseFile')}
              className="peer sr-only"
              onChange={(e) => acceptFile(e.target.files?.[0])}
            />
            <label
              htmlFor={ZIP_INPUT_ID}
              className="inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md border border-input bg-background px-3 text-xs font-medium transition-colors hover:bg-muted peer-focus-visible:ring-2 peer-focus-visible:ring-ring peer-focus-visible:ring-offset-1 peer-focus-visible:ring-offset-background"
            >
              <FolderOpen aria-hidden className="h-3.5 w-3.5" />
              {selectedFile ? t('form.replaceFile') : t('form.chooseFile')}
            </label>
          </div>
          <p className="text-2xs text-muted-foreground">{t('form.packageHint')}</p>
          <FormError message={visibleError} />
          <Button type="submit" disabled={isPending || !selectedFile}>
            {importPackage.isPending ? t('form.importing') : t('form.importPackage')}
          </Button>
        </form>
      </TabsContent>

      <TabsContent value="github">
        <form onSubmit={onSubmitGithub} className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="skill-github-url">{t('form.repository')}</Label>
            <Input
              id="skill-github-url"
              placeholder={t('form.repositoryPlaceholder')}
              value={githubUrl}
              onChange={(e) => setGithubUrl(e.target.value)}
            />
          </div>
          <div className="grid gap-3 sm:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
            <div className="space-y-1.5">
              <Label htmlFor="skill-github-branch">{t('form.branch')}</Label>
              <Input
                id="skill-github-branch"
                placeholder="main"
                value={githubBranch}
                onChange={(e) => setGithubBranch(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="skill-github-path">{t('form.skillPath')}</Label>
              <Input
                id="skill-github-path"
                placeholder="skills/calculator"
                value={githubPath}
                onChange={(e) => setGithubPath(e.target.value)}
              />
            </div>
          </div>
          <p className="text-2xs text-muted-foreground">
            {t('form.repositoryHint')}
          </p>
          <FormError message={visibleError} />
          <Button type="submit" disabled={isPending || !githubUrl.trim()}>
            {importGithub.isPending ? t('form.installing') : t('form.fetchInstall')}
          </Button>
        </form>
      </TabsContent>

      <TabsContent value="markdown">
        <form onSubmit={onSubmitMd} className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="skill-raw">{t('form.paste')}</Label>
            <Textarea
              id="skill-raw"
              rows={14}
              spellCheck={false}
              className="font-mono text-xs"
              placeholder={t('form.rawPlaceholder')}
              value={raw}
              onChange={(e) => setRaw(e.target.value)}
            />
            <p className="text-2xs text-muted-foreground">
              {t('form.markdownHint')}
            </p>
          </div>
          <FormError message={visibleError} />
          <Button type="submit" disabled={isPending}>
            {importSkill.isPending ? t('form.importing') : t('form.importSkill')}
          </Button>
        </form>
      </TabsContent>
    </Tabs>
  )
}
