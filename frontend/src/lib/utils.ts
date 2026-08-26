import { type ClassValue, clsx } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatLatency(ms: number): string {
  return `${ms.toFixed(0)}ms`;
}

export function formatThroughput(rate: number): string {
  if (rate >= 1000) {
    return `${(rate / 1000).toFixed(1)}K/min`;
  }
  return `${rate.toFixed(0)}/min`;
}

export function formatPercentage(value: number): string {
  return `${(value * 100).toFixed(2)}%`;
}

export function formatTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  return date.toLocaleTimeString('en-US', { 
    hour: '2-digit', 
    minute: '2-digit', 
    second: '2-digit',
    hour12: false 
  });
}

export function formatRelativeTime(timestamp: string): string {
  const now = Date.now();
  const then = new Date(timestamp).getTime();
  const diff = now - then;
  
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return `${seconds}s ago`;
  
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function getWorkloadStateColor(state: string): string {
  const colors: Record<string, string> = {
    HEALTHY: 'text-success',
    DEGRADED: 'text-warning',
    OVERLOADED: 'text-error',
    RECOVERING: 'text-info',
  };
  return colors[state] || 'text-text-secondary';
}

export function getActionTypeLabel(actionType: string): string {
  const labels: Record<string, string> = {
    CREATE_REPLICA: 'Create Replica',
    SHIFT_ROUTE: 'Shift Route',
    MIGRATE_PARTITION: 'Migrate Partition',
    THROTTLE_WORKLOAD: 'Throttle Workload',
    ROLLBACK: 'Rollback',
  };
  return labels[actionType] || actionType;
}

export function interpolateValue(from: number, to: number, progress: number): number {
  return from + (to - from) * progress;
}
