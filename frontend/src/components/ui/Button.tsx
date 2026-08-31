import { ButtonHTMLAttributes, ReactNode } from 'react';
import { cn } from '@/lib/utils';

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger' | 'tab' | 'tab-active';
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
    primary:
      'bg-accent text-[#1D1E1C] font-extrabold hover:bg-accent-hover active:translate-y-[1px] shadow-sm',
    secondary:
      'bg-charcoal text-text-primary font-bold hover:bg-charcoal-hover active:translate-y-[1px]',
    ghost:
      'bg-transparent text-text-secondary hover:text-text-primary hover:bg-surface-hover',
    danger:
      'bg-error text-white font-bold hover:bg-error/90',
    tab:
      'bg-transparent text-text-secondary hover:text-text-primary rounded-tab',
    'tab-active':
      'bg-surface-active text-text-primary font-bold rounded-tab shadow-inner',
  };

  const sizeStyles = {
    sm: 'px-4 py-2 text-small rounded-full',
    md: 'px-6 py-3 text-body rounded-full min-h-[44px]',
    lg: 'px-8 py-4 text-body font-bold rounded-full min-h-[58px]',
  };

  return (
    <button
      className={cn(
        'inline-flex items-center justify-center font-sans transition-all duration-150',
        'focus:outline-none focus:ring-2 focus:ring-accent/50 focus:ring-offset-2 focus:ring-offset-background',
        'disabled:opacity-40 disabled:cursor-not-allowed active:scale-[0.99]',
        variantStyles[variant],
        variant.startsWith('tab') ? 'px-4 py-2 text-small' : sizeStyles[size],
        className
      )}
      {...props}
    >
      {children}
    </button>
  );
}

