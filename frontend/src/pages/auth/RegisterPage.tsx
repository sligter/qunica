import { useEffect } from 'react'
import { useTranslation } from 'react-i18next'
import { Link, useNavigate } from 'react-router-dom'

import { AuthForm } from '@/components/auth/AuthForm'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

export function RegisterPage() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const title = t('register.title')

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
          <AuthForm mode="register" onSuccess={() => void navigate('/')} />
          <p className="text-sm text-muted-foreground">
            {t('register.switchPrompt')}{' '}
            <Link to="/login" className="font-medium text-foreground hover:underline">
              {t('register.switchAction')}
            </Link>
          </p>
        </CardContent>
      </Card>
    </div>
  )
}
