import { cn } from '@/lib/utils'

export interface SegmentedOption<T extends string> {
  value: T
  label: string
}

interface SegmentedControlProps<T extends string> {
  options: SegmentedOption<T>[]
  value: T
  onChange: (value: T) => void
  disabled?: boolean
  ariaLabel?: string
}

/** 同一设置项的互斥选项(复刻 cc-switch TabsTrigger:选中=实心蓝底白字)。 */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  disabled,
  ariaLabel = '选项',
}: SegmentedControlProps<T>) {
  return (
    <div
      role="radiogroup"
      aria-label={ariaLabel}
      data-tauri-drag-region="false"
      className={cn(
        'inline-flex h-8 items-center gap-1 rounded-lg bg-muted p-1',
        disabled && 'pointer-events-none opacity-50',
      )}
    >
      {options.map((opt) => {
        const active = value === opt.value
        return (
          <button
            key={opt.value}
            role="radio"
            aria-checked={active}
            data-tauri-drag-region="false"
            onClick={() => onChange(opt.value)}
            className={cn(
              'flex h-full items-center rounded-md px-3 text-[13px] font-medium transition-all duration-200',
              active
                ? 'bg-blue-500 text-white shadow-sm dark:bg-blue-600'
                : 'text-muted-foreground opacity-70 hover:opacity-100 hover:text-foreground',
            )}
          >
            {opt.label}
          </button>
        )
      })}
    </div>
  )
}
