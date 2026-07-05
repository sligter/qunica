import { useNavigate } from 'react-router-dom'

import { ImportSkillForm } from '@/components/skills/ImportSkillForm'
import { DetailShell } from '@/components/layout/DetailShell'

export function SkillCreatePage() {
  const navigate = useNavigate()
  return (
    <DetailShell
      title="Import a skill"
      subtitle="Import a skill from a zip package, GitHub repository, or Anthropic-style SKILL.md. When mounted on an agent, the body is appended to that agent's system prompt for every invocation."
    >
      <ImportSkillForm onCreated={(id) => void navigate(`/skills/${id}`)} />
    </DetailShell>
  )
}
