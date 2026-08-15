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
}

/** 顶部分段切换器(普通运行 / 开发模式 / 维护)。 */
export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  disabled,
}: SegmentedControlProps<T>) {
  return (
    <div
      role="tablist"
      data-tauri-drag-region="false"
      className={cn(
        'inline-flex h-9 items-center gap-0.5 rounded-[var(--radius-control)] border border-[var(--border)] bg-[var(--muted)] p-0.5',
        disabled && 'pointer-events-none opacity-50',
      )}
    >
      {options.map((opt) => (
        <button
          key={opt.value}
          role="tab"
          aria-selected={value === opt.value}
          data-tauri-drag-region="false"
          onClick={() => onChange(opt.value)}
          className={cn(
            'h-full rounded-[10px] px-3.5 text-[13px] font-medium transition-colors',
            value === opt.value
              ? 'bg-[var(--card)] text-[var(--foreground)] shadow-sm'
              : 'text-[var(--muted-foreground)] hover:text-[var(--foreground)]',
          )}
        >
          {opt.label}
        </button>
      ))}
    </div>
  )
}
