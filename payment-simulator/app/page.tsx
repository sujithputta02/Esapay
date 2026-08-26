import PaymentSimulator from './client-page';

async function getInitialWorkloads() {
  try {
    const res = await fetch('http://localhost:8080/api/workloads', {
      headers: { 'Content-Type': 'application/json' },
    });
    if (res.ok) {
      return await res.json();
    }
  } catch (error) {
    console.error('Failed to fetch initial workloads:', error);
  }
  return [];
}

export default async function Page() {
  const initialWorkloads = await getInitialWorkloads();
  return <PaymentSimulator initialWorkloads={initialWorkloads} />;
}
