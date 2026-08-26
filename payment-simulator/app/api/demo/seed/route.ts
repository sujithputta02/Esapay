export async function POST() {
  try {
    const response = await fetch('http://localhost:8080/api/demo/seed', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
    });
    const data = await response.json();
    return Response.json(data);
  } catch (error) {
    console.error('Failed to seed demo data:', error);
    return Response.json({ error: 'Failed to seed demo data' }, { status: 500 });
  }
}
