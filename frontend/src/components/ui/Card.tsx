import { ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface CardProps {
  children: ReactNode;
  className?: string;
  glass?: boolean;
  hover?: boolean;
}

export function Card({ children, className, glass = false, hover = false }: CardProps) {
  return (
    <div
      className={cn(
        'rounded-lg border transition-all duration-200',
        glass
          ? 'bg-background-card/40 backdrop-blur-md border-border'
          : 'bg-background-card border-border',
        hover && 'hover:border-border-hover hover:shadow-lg',
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
    <div className={cn('px-6 py-4 border-b border-border', className)}>
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
    <div className={cn('px-6 py-4', className)}>
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
    <h3 className={cn('text-h4 text-text-primary font-semibold', className)}>
      {children}
    </h3>
  );
}
