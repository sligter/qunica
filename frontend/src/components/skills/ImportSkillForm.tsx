import { useRef, useState } from 'react'
import { FileArchive, Github, Upload } from 'lucide-react'

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

interface ImportSkillFormProps {
  onCreated?: (newSkillId: string) => void
}

const PLACEHOLDER = `---
name: my-skill
description: One-line summary of what this skill does.
---

# My Skill

The body is markdown. It will be appended to the agent's system prompt
verbatim when this skill is mounted on the agent.
`

export function ImportSkillForm({ onCreated }: ImportSkillFormProps = {}) {
  const importSkill = useImportSkill()
  const importPackage = useImportSkillPackage()
  const importGithub = useImportSkillFromGithub()
  const [raw, setRaw] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [selectedFile, setSelectedFile] = useState<File | null>(null)
  const [githubUrl, setGithubUrl] = useState('')
  const [githubBranch, setGithubBranch] = useState('main')
  const [githubPath, setGithubPath] = useState('')
  const fileInputRef = useRef<HTMLInputElement>(null)

  const onSubmitMd = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    if (!raw.trim()) {
      setError('Paste a SKILL.md before submitting.')
      return
    }
    try {
      const created = await importSkill.mutateAsync({ raw })
      setRaw('')
      onCreated?.(created.id)
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Network error')
    }
  }

  const onSubmitZip = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    if (!selectedFile) {
      setError('Select a .zip file first.')
      return
    }
    try {
      const created = await importPackage.mutateAsync(selectedFile)
      setSelectedFile(null)
      if (fileInputRef.current) fileInputRef.current.value = ''
      onCreated?.(created.id)
    } catch (err) {
      setError(err instanceof ApiError ? err.message : 'Network error')
    }
  }

  const onSubmitGithub = async (e: React.FormEvent) => {
    e.preventDefault()
    setError(null)
    if (!githubUrl.trim()) {
      setError('Enter a GitHub repository URL first.')
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
      setError(err instanceof ApiError ? err.message : 'Network error')
    }
  }

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault()
    const file = e.dataTransfer.files[0]
    if (file && file.name.endsWith('.zip')) {
      setSelectedFile(file)
      setError(null)
    } else {
      setError('Only .zip files are accepted.')
    }
  }

  const isPending = importSkill.isPending || importPackage.isPending || importGithub.isPending

  return (
    <Tabs defaultValue="package" className="space-y-4">
      <TabsList className="w-full">
        <TabsTrigger value="package" className="flex-1">
          <FileArchive className="mr-1.5 h-3.5 w-3.5" />
          Package (.zip)
        </TabsTrigger>
        <TabsTrigger value="markdown" className="flex-1">
          Paste SKILL.md
        </TabsTrigger>
        <TabsTrigger value="github" className="flex-1">
          <Github className="mr-1.5 h-3.5 w-3.5" />
          GitHub
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
                {selectedFile ? selectedFile.name : 'Drop a .zip skill package here'}
              </p>
              <p className="text-xs text-muted-foreground">
                {selectedFile
                  ? `${(selectedFile.size / 1024).toFixed(1)} KB`
                  : 'or click to browse'}
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
            The zip must contain a <code>SKILL.md</code> file with YAML frontmatter.
            Optional: <code>scripts/</code>, <code>references/</code>, <code>assets/</code> directories.
          </p>
          {error && (
            <p className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <Button type="submit" disabled={isPending || !selectedFile}>
            {importPackage.isPending ? 'Importing…' : 'Import package'}
          </Button>
        </form>
      </TabsContent>

      <TabsContent value="github">
        <form onSubmit={onSubmitGithub} className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="skill-github-url">GitHub repository</Label>
            <Input
              id="skill-github-url"
              placeholder="https://github.com/user/repo or user/repo"
              value={githubUrl}
              onChange={(e) => setGithubUrl(e.target.value)}
            />
          </div>
          <div className="grid gap-3 sm:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
            <div className="space-y-1.5">
              <Label htmlFor="skill-github-branch">Branch</Label>
              <Input
                id="skill-github-branch"
                placeholder="main"
                value={githubBranch}
                onChange={(e) => setGithubBranch(e.target.value)}
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="skill-github-path">Skill path</Label>
              <Input
                id="skill-github-path"
                placeholder="skills/calculator"
                value={githubPath}
                onChange={(e) => setGithubPath(e.target.value)}
              />
            </div>
          </div>
          <p className="text-[11px] text-muted-foreground">
            The repository, or selected directory, must contain a <code>SKILL.md</code>.
          </p>
          {error && (
            <p className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <Button type="submit" disabled={isPending || !githubUrl.trim()}>
            {importGithub.isPending ? 'Installing...' : 'Fetch and install'}
          </Button>
        </form>
      </TabsContent>

      <TabsContent value="markdown">
        <form onSubmit={onSubmitMd} className="space-y-4">
          <div className="space-y-1.5">
            <Label htmlFor="skill-raw">Paste SKILL.md</Label>
            <Textarea
              id="skill-raw"
              rows={14}
              spellCheck={false}
              className="font-mono text-xs"
              placeholder={PLACEHOLDER}
              value={raw}
              onChange={(e) => setRaw(e.target.value)}
            />
            <p className="text-[11px] text-muted-foreground">
              The file must start with YAML frontmatter (<code>---</code>) containing{' '}
              <code>name</code> and an optional <code>description</code>.
            </p>
          </div>
          {error && (
            <p className="text-sm text-destructive" role="alert">
              {error}
            </p>
          )}
          <Button type="submit" disabled={isPending}>
            {importSkill.isPending ? 'Importing…' : 'Import skill'}
          </Button>
        </form>
      </TabsContent>
    </Tabs>
  )
}
