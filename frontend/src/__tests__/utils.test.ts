import { describe, it, expect } from 'bun:test';
import {
  formatLatency,
  formatThroughput,
  formatPercentage,
  getWorkloadStateColor,
  getActionTypeLabel,
} from '../lib/utils';

describe('formatLatency', () => {
  it('formats latency in milliseconds', () => {
    expect(formatLatency(123.456)).toBe('123ms');
    expect(formatLatency(50.1)).toBe('50ms');
    expect(formatLatency(0)).toBe('0ms');
  });
});

describe('formatThroughput', () => {
  it('formats small throughput', () => {
    expect(formatThroughput(500)).toBe('500/min');
  });

  it('formats large throughput in K', () => {
    expect(formatThroughput(1500)).toBe('1.5K/min');
    expect(formatThroughput(10000)).toBe('10.0K/min');
  });
});

describe('formatPercentage', () => {
  it('formats percentage values', () => {
    expect(formatPercentage(0.01)).toBe('1.00%');
    expect(formatPercentage(0.5)).toBe('50.00%');
    expect(formatPercentage(0.99)).toBe('99.00%');
  });
});

describe('getWorkloadStateColor', () => {
  it('returns correct color class for each state', () => {
    expect(getWorkloadStateColor('HEALTHY')).toBe('text-success');
    expect(getWorkloadStateColor('DEGRADED')).toBe('text-warning');
    expect(getWorkloadStateColor('OVERLOADED')).toBe('text-error');
    expect(getWorkloadStateColor('RECOVERING')).toBe('text-info');
  });

  it('returns default color for unknown state', () => {
    expect(getWorkloadStateColor('UNKNOWN')).toBe('text-text-secondary');
  });
});

describe('getActionTypeLabel', () => {
  it('returns human-readable labels', () => {
    expect(getActionTypeLabel('CREATE_REPLICA')).toBe('Create Replica');
    expect(getActionTypeLabel('SHIFT_ROUTE')).toBe('Shift Route');
    expect(getActionTypeLabel('ROLLBACK')).toBe('Rollback');
  });

  it('returns the action type for unknown types', () => {
    expect(getActionTypeLabel('UNKNOWN_ACTION')).toBe('UNKNOWN_ACTION');
  });
});
