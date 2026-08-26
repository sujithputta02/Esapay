import { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger';
  size?: 'sm' | 'md' | 'lg';
}

export function Button({
  children,
  variant = 'primary',
  size = 'md',
  className,
  ...props
}: ButtonProps) {
  const variantStyles = {
    primary: 'bg-accent text-background hover:bg-accent-light',
    secondary: 'bg-background-elevated text-text-primary hover:bg-background-card border border-border',
    ghost: 'bg-transparent text-text-primary hover:bg-background-elevated',
    danger: 'bg-error text-white hover:bg-error/90',
  };

  const sizeStyles = {
    sm: 'px-3 py-1.5 text-small',
    md: 'px-4 py-2 text-body',
    lg: 'px-6 py-3 text-body',
  };

  return (
    <button
      className={cn(
        'inline-flex items-center justify-center rounded-md font-medium transition-all duration-200',
        'focus:outline-none focus:ring-2 focus:ring-accent focus:ring-offset-2 focus:ring-offset-background',
        'disabled:opacity-50 disabled:cursor-not-allowed',
        variantStyles[variant],
        sizeStyles[size],
        className
      )}
      {...props}
    >
      {children}
    </button>
  );
}
