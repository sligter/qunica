import { Dialog, DialogContent, DialogTitle } from '@/components/ui/dialog'

interface ImageLightboxProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  src: string | null
  alt: string
}

export function ImageLightbox({ open, onOpenChange, src, alt }: ImageLightboxProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        closeLabel="Close image preview"
        aria-describedby={undefined}
        className="w-[min(96vw,1100px)] max-w-none border-0 bg-transparent p-2 shadow-none"
      >
        <DialogTitle className="sr-only">{alt}</DialogTitle>
        {src ? (
          <img
            src={src}
            alt={alt}
            className="max-h-[88vh] w-full object-contain"
          />
        ) : null}
      </DialogContent>
    </Dialog>
  )
}
