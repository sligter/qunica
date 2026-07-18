import { useRef, useState } from 'react'
import { FileArchive, Github, Upload } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Textarea } from '@/components/ui/textarea'
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

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const file = e.dataTransfer.files[0]
    if (file && file.name.endsWith('.zip')) {
      setSelectedFile(file)
      setError(null)
    } else {
      setError(translatedError('errors.zipOnly'))
    }
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
            className="flex flex-col items-center justify-center gap-3 rounded-lg border-2 border-dashed border-border p-8 text-center transition-colors hover:border-primary/40"
            onDragOver={(e) => e.preventDefault()}
            onDrop={handleDrop}
          >
            <Upload className="h-8 w-8 text-muted-foreground" />
            <div>
              <p className="text-sm font-medium">
                {selectedFile ? selectedFile.name : t('form.dropPackage')}
              </p>
              <p className="text-xs text-muted-foreground">
                {selectedFile
                  ? `${(selectedFile.size / 1024).toFixed(1)} KB`
                  : t('form.browse')}
              </p>
            </div>
            <input
              ref={fileInputRef}
              type="file"
              accept=".zip"
              className="absolute inset-0 cursor-pointer opacity-0"
              style={{ position: 'relative' }}
              onChange={(e) => {
                const f = e.target.files?.[0]
                if (f) {
                  setSelectedFile(f)
                  setError(null)
                }
              }}
            />
          </div>
          <p className="text-[11px] text-muted-foreground">
            {t('form.packageHint')}
          </p>
          {visibleError && (
            <p className="text-sm text-destructive" role="alert">
              {visibleError}
            </p>
          )}
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
          <p className="text-[11px] text-muted-foreground">
            {t('form.repositoryHint')}
          </p>
          {visibleError && (
            <p className="text-sm text-destructive" role="alert">
              {visibleError}
            </p>
          )}
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
            <p className="text-[11px] text-muted-foreground">
              {t('form.markdownHint')}
            </p>
          </div>
          {visibleError && (
            <p className="text-sm text-destructive" role="alert">
              {visibleError}
            </p>
          )}
          <Button type="submit" disabled={isPending}>
            {importSkill.isPending ? t('form.importing') : t('form.importSkill')}
          </Button>
        </form>
      </TabsContent>
    </Tabs>
  )
}
