// ESA Design Token System

export const designTokens = {
  radius: {
    sm: '0.5rem',    // 8px
    md: '0.75rem',   // 12px
    lg: '1rem',      // 16px
    xl: '1.5rem',    // 24px
  },
  
  motion: {
    fast: 0.15,
    normal: 0.24,
    slow: 0.45,
    cinematic: 1.2,
  },
  
  spacing: {
    xs: 4,
    sm: 8,
    md: 16,
    lg: 24,
    xl: 32,
    xxl: 48,
  },
  
  typography: {
    display: { size: '3.5rem', weight: 700, lineHeight: 1.1 },
    h1: { size: '2.5rem', weight: 600, lineHeight: 1.2 },
    h2: { size: '2rem', weight: 600, lineHeight: 1.3 },
    h3: { size: '1.5rem', weight: 600, lineHeight: 1.4 },
    h4: { size: '1.25rem', weight: 600, lineHeight: 1.5 },
    body: { size: '1rem', weight: 400, lineHeight: 1.6 },
    small: { size: '0.875rem', weight: 400, lineHeight: 1.5 },
    micro: { size: '0.75rem', weight: 400, lineHeight: 1.4 },
  },
  
  colors: {
    background: {
      primary: '#0a0a0b',
      elevated: '#121214',
      surface: '#1a1a1d',
      card: '#202023',
    },
    text: {
      primary: '#f5f5f6',
      secondary: '#a1a1a6',
      muted: '#6e6e73',
    },
    accent: {
      primary: '#f59e0b',
      light: '#fbbf24',
      dark: '#d97706',
    },
    semantic: {
      success: '#10b981',
      warning: '#f59e0b',
      error: '#ef4444',
      info: '#3b82f6',
    },
    border: {
      primary: 'rgba(255, 255, 255, 0.08)',
      hover: 'rgba(255, 255, 255, 0.12)',
    },
  },
  
  shadows: {
    sm: '0 1px 2px 0 rgba(0, 0, 0, 0.05)',
    md: '0 4px 6px -1px rgba(0, 0, 0, 0.1)',
    lg: '0 10px 15px -3px rgba(0, 0, 0, 0.1)',
    xl: '0 20px 25px -5px rgba(0, 0, 0, 0.1)',
  },
};

export const stateColors = {
  HEALTHY: '#10b981',
  DEGRADED: '#f59e0b',
  OVERLOADED: '#ef4444',
  RECOVERING: '#3b82f6',
};

export const severityColors = {
  LOW: '#3b82f6',
  MEDIUM: '#f59e0b',
  HIGH: '#ef4444',
  CRITICAL: '#dc2626',
};

export const agentColors = {
  monitor: '#3b82f6',
  diagnosis: '#8b5cf6',
  planning: '#f59e0b',
  safety: '#10b981',
};
