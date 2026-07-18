import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router-dom'

import { AuthForm } from '@/components/auth/AuthForm'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

export function LoginPage() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const title = t('login.title')

  useEffect(() => {
    document.title = title
  }, [title])

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-6">
      <Card className="w-full max-w-sm">
        <CardHeader>
          <CardTitle>{title}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <AuthForm mode="login" onSuccess={() => void navigate('/')} />
          <p className="text-sm text-muted-foreground">
            {t('login.switchPrompt')}{' '}
            <Link to="/register" className="font-medium text-foreground hover:underline">
              {t('login.switchAction')}
            </Link>
          </p>
        </CardContent>
      </Card>
    </div>
  )
}
