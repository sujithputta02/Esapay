import { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface BadgeProps {
  children: ReactNode;
  variant?: 'default' | 'success' | 'warning' | 'error' | 'info' | 'accent' | 'charcoal';
  className?: string;
}

export function Badge({ children, variant = 'default', className }: BadgeProps) {
  const variantStyles = {
    default: 'bg-surface-raised text-text-secondary border-border',
    success: 'bg-accent/15 text-accent font-semibold border-accent/25',
    accent: 'bg-accent/15 text-accent font-semibold border-accent/25',
    warning: 'bg-warning/15 text-warning font-semibold border-warning/25',
    error: 'bg-error/15 text-error font-semibold border-error/25',
    info: 'bg-info/15 text-info font-semibold border-info/25',
    charcoal: 'bg-charcoal text-text-primary border-border/50',
  };

  return (
    <span
      className={cn(
        'inline-flex items-center px-3 py-0.5 rounded-full text-micro font-medium border transition-colors',
        variantStyles[variant],
        className
      )}
    >
      {children}
    </span>
  );
}

