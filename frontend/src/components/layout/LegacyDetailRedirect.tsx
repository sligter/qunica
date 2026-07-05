import { Navigate, useParams } from 'react-router-dom'

interface LegacyDetailRedirectProps {
  /** New base path, e.g. "/agents". */
  base: string
}

/** Redirect a legacy `/settings/area/:id` deep link to its `/area/:id` home. */
export function LegacyDetailRedirect({ base }: LegacyDetailRedirectProps) {
  const { id } = useParams<{ id: string }>()
  return <Navigate to={id ? `${base}/${id}` : base} replace />
}
