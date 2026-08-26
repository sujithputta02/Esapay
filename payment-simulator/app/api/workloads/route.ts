export async function GET() {
  try {
    const response = await fetch('http://localhost:8080/api/workloads', {
      headers: {
        'Content-Type': 'application/json',
      },
    });
    const data = await response.json();
    return Response.json(data);
  } catch (error) {
    console.error('Failed to fetch workloads from backend:', error);
    return Response.json({ error: 'Failed to fetch workloads' }, { status: 500 });
  }
}
