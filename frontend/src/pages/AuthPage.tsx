import { useLocation } from 'react-router-dom';
import { EnterpriseAuthDialog } from '@/components/EnterpriseAuthDialog';

export function AuthPage() {
  const location = useLocation();
  const searchParams = new URLSearchParams(location.search);
  const isSignup =
    location.pathname.includes('/signup') ||
    location.pathname.includes('/register') ||
    searchParams.get('mode') === 'signup' ||
    searchParams.get('auth') === 'signup';

  return (
    <EnterpriseAuthDialog
      isOpen={true}
      onClose={() => {}}
      initialMode={isSignup ? 'signup' : 'signin'}
      isStandalonePage={true}
    />
  );
}
