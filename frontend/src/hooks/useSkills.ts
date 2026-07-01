import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { ApiError, fetchJson } from '@/lib/api-v2/client'
import { useAuthStore } from '@/stores/authStore'
import type {
  SkillCreate,
  SkillGithubImport,
  SkillImport,
  SkillRead,
  SkillResourceRead,
} from '@/types/api'

function encodeResourcePath(path: string) {
  return path.split('/').map(encodeURIComponent).join('/')
}

function readFileAsBase64(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader()

    reader.onload = () => {
      if (typeof reader.result !== 'string') {
        reject(new ApiError(0, 'file_read_error', 'Unable to read skill package file.'))
        return
      }

      const base64Start = reader.result.indexOf(',')
      resolve(base64Start === -1 ? reader.result : reader.result.slice(base64Start + 1))
    }

    reader.onerror = () => {
      reject(
        new ApiError(
          0,
          'file_read_error',
          reader.error?.message ?? 'Unable to read skill package file.',
        ),
      )
    }

    reader.readAsDataURL(file)
  })
}

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

export function useSkillResources(skillId: string | undefined) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['skills', skillId, 'resources'],
    queryFn: () => fetchJson<SkillResourceRead[]>(`/skills/${skillId}/resources`, { token }),
    enabled: token !== null && !!skillId,
  })
}

export function useSkillResource(skillId: string | undefined, path: string | null) {
  const token = useAuthStore((s) => s.token)
  return useQuery({
    queryKey: ['skills', skillId, 'resources', path],
    queryFn: () =>
      fetchJson<SkillResourceRead>(
        `/skills/${skillId}/resources/${encodeResourcePath(path ?? '')}`,
        { token },
      ),
    enabled: token !== null && !!skillId && !!path,
  })
}

export function useUpdateSkillResource(skillId: string | undefined, path: string | null) {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (content: string) =>
      fetchJson<SkillResourceRead>(
        `/skills/${skillId}/resources/${encodeResourcePath(path ?? '')}`,
        { token, method: 'PATCH', body: { content } },
      ),
    onSuccess: (updated) => {
      void qc.invalidateQueries({ queryKey: ['skills', skillId, 'resources'] })
      qc.setQueryData(['skills', skillId, 'resources', updated.path], updated)
      void qc.invalidateQueries({ queryKey: ['skills'] })
      void qc.invalidateQueries({ queryKey: ['skills', skillId] })
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
    mutationFn: async (file: File) => {
      const contentBase64 = await readFileAsBase64(file)
      return fetchJson<SkillRead>('/skills/import-package', {
        token,
        method: 'POST',
        body: {
          filename: file.name,
          content_base64: contentBase64,
        },
      })
    },
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['skills'] })
    },
  })
}

export function useImportSkillFromGithub() {
  const token = useAuthStore((s) => s.token)
  const qc = useQueryClient()
  return useMutation({
    mutationFn: (data: SkillGithubImport) =>
      fetchJson<SkillRead>('/skills/import-github', {
        token,
        method: 'POST',
        body: data,
      }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['skills'] })
    },
  })
}
