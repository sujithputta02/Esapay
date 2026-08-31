import { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface CardProps {
  children: ReactNode;
  className?: string;
  variant?: 'surface' | 'raised' | 'control' | 'action';
  hover?: boolean;
}

export function Card({
  children,
  className,
  variant = 'surface',
  hover = false,
}: CardProps) {
  const variantStyles = {
    surface: 'bg-surface rounded-[32px] border border-border/50',
    raised: 'bg-surface-raised rounded-[22px] border border-border/30',
    control: 'bg-surface-control rounded-[20px]',
    action: 'bg-surface rounded-[40px] border border-border/50',
  };

  return (
    <div
      className={cn(
        'transition-colors duration-150',
        variantStyles[variant],
        hover && 'hover:bg-surface-hover',
        className
      )}
    >
      {children}
    </div>
  );
}

interface CardHeaderProps {
  children: ReactNode;
  className?: string;
}

export function CardHeader({ children, className }: CardHeaderProps) {
  return (
    <div className={cn('px-8 py-5 border-b border-border/40', className)}>
      {children}
    </div>
  );
}

interface CardBodyProps {
  children: ReactNode;
  className?: string;
}

export function CardBody({ children, className }: CardBodyProps) {
  return (
    <div className={cn('px-8 py-6', className)}>
      {children}
    </div>
  );
}

interface CardTitleProps {
  children: ReactNode;
  className?: string;
}

export function CardTitle({ children, className }: CardTitleProps) {
  return (
    <h3 className={cn('text-h3 font-bold text-text-primary tracking-tight', className)}>
      {children}
    </h3>
  );
}

