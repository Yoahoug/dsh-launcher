import * as React from 'react'
import { cva, type VariantProps } from 'class-variance-authority'
import { cn } from '@/lib/utils'

/** 复刻 cc-switch 状态徽章:浅色底 + 深色字,深色模式半透明底。 */
const badgeVariants = cva(
  'inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-[11px] font-semibold',
  {
    variants: {
      variant: {
        success:
          'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300',
        warning:
          'bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-300',
        danger:
          'bg-red-100 text-red-700 dark:bg-red-900/40 dark:text-red-300',
        neutral:
          'bg-gray-100 text-gray-600 dark:bg-gray-800 dark:text-gray-300',
        primary:
          'bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300',
        info: 'bg-sky-100 text-sky-700 dark:bg-sky-900/40 dark:text-sky-300',
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
