import { useNavigate } from 'react-router-dom'

import { ImportSkillForm } from '@/components/skills/ImportSkillForm'

export function CreateSkillRightPane() {
  const navigate = useNavigate()
  return (
    <div className="flex h-full w-full flex-col overflow-y-auto bg-background">
      <div className="mx-auto w-full max-w-2xl space-y-4 p-8">
        <header className="space-y-1">
          <h1 className="text-xl font-semibold tracking-tight">Import a skill</h1>
          <p className="text-sm text-muted-foreground">
            Import a skill from a zip package, GitHub repository, or Anthropic-style
            SKILL.md. When mounted on an agent, the body is appended to that agent's
            system prompt for every invocation.
          </p>
        </header>
        <ImportSkillForm onCreated={(id) => void navigate(`/skills/${id}`)} />
      </div>
    </div>
  )
}
