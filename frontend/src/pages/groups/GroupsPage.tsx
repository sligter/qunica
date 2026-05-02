import { Link, useNavigate } from 'react-router-dom'

import { CreateGroupForm } from '@/components/groups/CreateGroupForm'
import { Button } from '@/components/ui/button'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from '@/components/ui/card'
import { useGroups } from '@/hooks/useGroups'

export function GroupsPage() {
  const groups = useGroups()
  const navigate = useNavigate()

  return (
    <div className="mx-auto max-w-5xl space-y-8 p-6">
      <section className="space-y-3">
        <h1 className="text-xl font-semibold tracking-tight">Your groups</h1>
        {groups.isLoading && <p className="text-sm text-muted-foreground">Loading…</p>}
        {groups.error && (
          <p className="text-sm text-red-600">Failed to load: {String(groups.error)}</p>
        )}
        {groups.data && groups.data.length === 0 && (
          <p className="text-sm text-muted-foreground">
            No groups yet — create your first one below.
          </p>
        )}
        {groups.data && groups.data.length > 0 && (
          <ul className="grid grid-cols-1 gap-3 md:grid-cols-2">
            {groups.data.map((g) => (
              <li key={g.id}>
                <Card>
                  <CardHeader>
                    <CardTitle className="text-base">{g.name}</CardTitle>
                    {g.description && <CardDescription>{g.description}</CardDescription>}
                  </CardHeader>
                  <CardContent>
                    <Button asChild size="sm" variant="outline">
                      <Link to={`/groups/${g.id}`}>Open</Link>
                    </Button>
                  </CardContent>
                </Card>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="space-y-3">
        <h2 className="text-lg font-semibold tracking-tight">New group</h2>
        <Card>
          <CardContent className="pt-6">
            <CreateGroupForm onCreated={(id) => void navigate(`/groups/${id}`)} />
          </CardContent>
        </Card>
      </section>
    </div>
  )
}
