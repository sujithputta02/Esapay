export async function POST(request: Request) {
  try {
    const body = await request.json();
    const response = await fetch('http://localhost:8080/api/demo/trigger-spike', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
      },
      body: JSON.stringify(body),
    });
    const data = await response.json();
    return Response.json(data);
  } catch (error) {
    console.error('Failed to trigger spike:', error);
    return Response.json({ error: 'Failed to trigger spike' }, { status: 500 });
  }
}
