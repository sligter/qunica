export const CONVERSATION_ID_MIME = 'application/x-qunica-conversation-id'

const MAX_CONVERSATION_ID_LENGTH = 200

export function setConversationIdDrag(dataTransfer: DataTransfer, conversationId: string): void {
  dataTransfer.effectAllowed = 'copy'
  dataTransfer.setData(CONVERSATION_ID_MIME, conversationId)
}

export function hasConversationIdDrag(dataTransfer: DataTransfer): boolean {
  return Array.from(dataTransfer.types).includes(CONVERSATION_ID_MIME)
}

export function conversationIdFromDataTransfer(dataTransfer: DataTransfer): string | null {
  const id = dataTransfer.getData(CONVERSATION_ID_MIME).trim()
  return id && id.length <= MAX_CONVERSATION_ID_LENGTH ? id : null
}
