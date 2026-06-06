import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'

interface ReasoningPassbackControlProps {
  value: boolean
  onChange: (value: boolean) => void
}

/**
 * Toggles whether the model's prior `reasoning_content` is re-sent on follow-up
 * turns of a multi-turn tool loop. Maps to the provider's `reasoning_passback`
 * setting (used by the OpenAI-compatible chat-model path).
 *
 * Reasoning models that support tool calling (e.g. DeepSeek, Xiaomi MiMo) expect
 * the thinking that produced a tool call to travel back with it. Defaults off
 * because plain chat models neither emit nor need it.
 */
export function ReasoningPassbackControl({ value, onChange }: ReasoningPassbackControlProps) {
  return (
    <div className="flex items-start justify-between gap-3">
      <div className="space-y-0.5">
        <Label>Reasoning passback</Label>
        <p className="text-[11px] leading-relaxed text-muted-foreground">
          Re-send the model&apos;s prior reasoning on multi-turn tool calls. Reasoning
          models with tool use (e.g. DeepSeek, MiMo) expect it. Enable for reasoning
          models; leave off for plain chat models.
        </p>
      </div>
      <Switch
        aria-label="Reasoning passback"
        checked={value}
        onCheckedChange={onChange}
        className="mt-1 shrink-0"
      />
    </div>
  )
}
