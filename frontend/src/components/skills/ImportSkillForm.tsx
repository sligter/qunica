import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import { useImportSkill } from '@/hooks/useSkills'
import { ApiError } from '@/lib/api'

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
  const [raw, setRaw] = useState('')
  const [error, setError] = useState<string | null>(null)

  const onSubmit = async (e: React.FormEvent) => {
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

  return (
    <form onSubmit={onSubmit} className="space-y-4">
      <div className="space-y-1.5">
        <Label htmlFor="skill-raw">Paste SKILL.md</Label>
        <Textarea
          id="skill-raw"
          rows={16}
          spellCheck={false}
          className="font-mono text-xs"
          placeholder={PLACEHOLDER}
          value={raw}
          onChange={(e) => setRaw(e.target.value)}
        />
        <p className="text-[11px] text-muted-foreground">
          The file must start with YAML frontmatter (<code>---</code>) containing{' '}
          <code>name</code> and an optional <code>description</code>. The body
          (markdown after the second <code>---</code>) is the skill's prompt
          fragment.
        </p>
      </div>
      {error && (
        <p className="text-sm text-red-600" role="alert">
          {error}
        </p>
      )}
      <Button type="submit" disabled={importSkill.isPending}>
        {importSkill.isPending ? 'Importing…' : 'Import skill'}
      </Button>
    </form>
  )
}
