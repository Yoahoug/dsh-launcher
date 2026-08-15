import * as React from 'react'
import { cn } from '@/lib/utils'

export interface SelectOption<T extends string> {
  value: T
  label: string
}

interface SelectProps<T extends string> extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'onChange'> {
  options: SelectOption<T>[]
  value: T
  onChange: (value: T) => void
}

/** 原生 select 包装(设置页枚举项)。 */
function Select<T extends string>({ options, value, onChange, className, ...props }: SelectProps<T>) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value as T)}
      className={cn(
        'h-9 w-full appearance-none rounded-[var(--radius-control)] border border-[var(--border)] bg-[var(--background)] px-3 pr-8 text-sm text-[var(--foreground)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--primary)]/40 disabled:opacity-50',
        className,
      )}
      {...props}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  )
}

export { Select }
