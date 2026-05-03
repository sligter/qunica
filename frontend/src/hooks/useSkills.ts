import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { fetchFormData, fetchJson } from '@/lib/api'
import { useAuthStore } from '@/stores/authStore'
import type { SkillCreate, SkillImport, SkillRead } from '@/types/api'

export function useSkills() {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['skills'],
    queryFn: () => fetchJson<SkillRead[]>('/skills', { token }),
    enabled: token !== null,
  })
}

export function useSkill(id: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['skills', id],
    queryFn: () => fetchJson<SkillRead>(`/skills/${id}`, { token }),
    enabled: token !== null && !!id,
  })
}

export function useImportSkill() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: SkillImport) =>
      fetchJson<SkillRead>('/skills/import', {
        token,
        method: 'POST',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['skills'] })
    },
  })
}

export function useCreateSkill() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: SkillCreate) =>
      fetchJson<SkillRead>('/skills', {
        token,
        method: 'POST',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['skills'] })
    },
  })
}

export function useDeleteSkill() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (id: string) =>
      fetchJson<void>(`/skills/${id}`, { token, method: 'DELETE' }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['skills'] })
    },
  })
}

export function useImportSkillPackage() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (file: File) => {
      const fd = new FormData()
      fd.append('file', file)
      return fetchFormData<SkillRead>('/skills/import-package', fd, { token })
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['skills'] })
    },
  })
}
