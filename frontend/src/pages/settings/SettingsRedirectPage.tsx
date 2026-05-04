import { Navigate } from 'react-router-dom'

export function SettingsRedirectPage() {
  return <Navigate to="/settings/system" replace />
}
