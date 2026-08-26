import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { QueryClientProvider } from '@tanstack/react-query';
import { Layout } from './components/Layout';
import { Dashboard } from './pages/Dashboard';
import { RuntimeView } from './pages/RuntimeView';
import { AgentsView } from './pages/AgentsView';
import { AuditView } from './pages/AuditView';
import { EffectsView } from './pages/EffectsView';
import { CostsView } from './pages/CostsView';
import { PolicyView } from './pages/PolicyView';
import { queryClient } from './lib/queryClient';

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<Layout />}>
            <Route index element={<Navigate to="/dashboard" replace />} />
            <Route path="dashboard" element={<Dashboard />} />
            <Route path="runtime" element={<RuntimeView />} />
            <Route path="agents" element={<AgentsView />} />
            <Route path="audit" element={<AuditView />} />
            <Route path="effects" element={<EffectsView />} />
            <Route path="costs" element={<CostsView />} />
            <Route path="policy" element={<PolicyView />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  );
}

export default App;
