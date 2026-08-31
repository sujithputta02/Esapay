/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        background: {
          DEFAULT: '#1D1E1C',
          shell: '#1D1E1C',
          elevated: '#272727',
          surface: '#272727',
          raised: '#333333',
          hover: '#3A3A39',
          active: '#4B4B4B',
          control: '#303030',
        },
        surface: {
          DEFAULT: '#272727',
          raised: '#333333',
          hover: '#3A3A39',
          active: '#4B4B4B',
          control: '#303030',
        },
        text: {
          primary: '#F5F5F5',
          secondary: '#B8B8B8',
          muted: '#777777',
          disabled: '#555555',
        },
        accent: {
          DEFAULT: '#C7F25C',
          hover: '#D7F873',
          strong: '#DDF47A',
          soft: 'rgba(199, 242, 92, 0.14)',
          focus: 'rgba(199, 242, 92, 0.35)',
        },
        charcoal: {
          DEFAULT: '#474745',
          hover: '#5A5A58',
        },
        success: '#C7F25C',
        warning: '#EAB308',
        error: '#EF4444',
        info: '#38BDF8',
        border: {
          DEFAULT: 'rgba(255, 255, 255, 0.06)',
          strong: 'rgba(255, 255, 255, 0.10)',
          hover: 'rgba(255, 255, 255, 0.12)',
        },
      },
      fontFamily: {
        sans: [
          'Inter',
          'ui-sans-serif',
          'system-ui',
          '-apple-system',
          'BlinkMacSystemFont',
          '"Segoe UI"',
          'sans-serif',
        ],
        mono: ['JetBrains Mono', 'ui-monospace', 'monospace'],
      },
      fontSize: {
        'display': ['2.25rem', { lineHeight: '1.2', fontWeight: '700' }],
        'h1': ['2rem', { lineHeight: '1.2', fontWeight: '700' }],
        'h2': ['1.75rem', { lineHeight: '1.25', fontWeight: '700' }],
        'h3': ['1.375rem', { lineHeight: '1.3', fontWeight: '600' }],
        'h4': ['1.125rem', { lineHeight: '1.4', fontWeight: '600' }],
        'body': ['1rem', { lineHeight: '1.6', fontWeight: '400' }],
        'small': ['0.875rem', { lineHeight: '1.5', fontWeight: '400' }],
        'micro': ['0.75rem', { lineHeight: '1.4', fontWeight: '400' }],
      },
      borderRadius: {
        'card': '32px',
        'stat': '22px',
        'action-bar': '40px',
        'tab': '16px',
        'pill': '999px',
      },
      boxShadow: {
        'floating': '0 16px 50px rgba(0,0,0,0.28)',
      },
    },
  },
  plugins: [],
}

