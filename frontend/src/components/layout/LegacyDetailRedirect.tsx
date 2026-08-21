import { Navigate, useLocation, useParams } from 'react-router-dom'

interface LegacyDetailRedirectProps {
  /** New base path, e.g. "/agents". */
  base: string
}

/**
 * Redirect a legacy `/settings/area/:id` deep link to its `/area/:id` home.
 * Passes `location.state` through so a link followed from a conversation keeps
 * its background location — the same contract as `OverlayRedirect`, just with a
 * computed destination.
 */
export function LegacyDetailRedirect({ base }: LegacyDetailRedirectProps) {
  const { id } = useParams<{ id: string }>()
  const location = useLocation()
  return <Navigate to={id ? `${base}/${id}` : base} replace state={location.state} />
}
