import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'

const badgeVariants = cva(
  'inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium',
  {
    variants: {
      variant: {
        success: 'bg-[var(--success)]/12 text-[var(--success)]',
        warning: 'bg-[var(--warning)]/14 text-[var(--warning)]',
        danger: 'bg-[var(--danger)]/12 text-[var(--danger)]',
        neutral: 'bg-[var(--muted)] text-[var(--muted-foreground)]',
        primary: 'bg-[var(--primary)]/12 text-[var(--primary)]',
      },
    },
    defaultVariants: { variant: 'neutral' },
  },
)

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />
}

/** 状态点(圆点 + 文案),运行态用绿点。 */
function StatusDot({ className }: { className?: string }) {
  return <span className={cn('size-1.5 shrink-0 rounded-full bg-current', className)} aria-hidden />
}

export { Badge, StatusDot, badgeVariants }
