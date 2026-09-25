import { BrowserRouter, Routes, Route } from 'react-router-dom';
import { QueryClientProvider } from '@tanstack/react-query';
import { Layout } from './components/Layout';
import { Dashboard } from './pages/Dashboard';
import { RuntimeView } from './pages/RuntimeView';
import { AgentsView } from './pages/AgentsView';
import { AuditView } from './pages/AuditView';
import { EffectsView } from './pages/EffectsView';
import { CostsView } from './pages/CostsView';
import { PolicyView } from './pages/PolicyView';
import { BenchmarksView } from './pages/BenchmarksView';
import { LandingPage } from './pages/LandingPage';
import { queryClient } from './lib/queryClient';

function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <BrowserRouter>
        <Routes>
          {/* ESA Command Center as the primary home route for server operators */}
          <Route path="/" element={<Layout />}>
            <Route index element={<Dashboard />} />
          </Route>
          <Route path="/dashboard" element={<Layout />}>
            <Route index element={<Dashboard />} />
          </Route>
          <Route path="/landing" element={<LandingPage />} />
          <Route path="/app" element={<Layout />}>
            <Route index element={<Dashboard />} />
            <Route path="dashboard" element={<Dashboard />} />
            <Route path="runtime" element={<RuntimeView />} />
            <Route path="agents" element={<AgentsView />} />
            <Route path="audit" element={<AuditView />} />
            <Route path="effects" element={<EffectsView />} />
            <Route path="costs" element={<CostsView />} />
            <Route path="policy" element={<PolicyView />} />
            <Route path="benchmarks" element={<BenchmarksView />} />
          </Route>
          <Route path="/runtime" element={<Layout />}><Route index element={<RuntimeView />} /></Route>
          <Route path="/agents" element={<Layout />}><Route index element={<AgentsView />} /></Route>
          <Route path="/audit" element={<Layout />}><Route index element={<AuditView />} /></Route>
          <Route path="/effects" element={<Layout />}><Route index element={<EffectsView />} /></Route>
          <Route path="/costs" element={<Layout />}><Route index element={<CostsView />} /></Route>
          <Route path="/policy" element={<Layout />}><Route index element={<PolicyView />} /></Route>
          <Route path="/benchmarks" element={<Layout />}><Route index element={<BenchmarksView />} /></Route>
        </Routes>
      </BrowserRouter>
    </QueryClientProvider>
  );
}

export default App;
