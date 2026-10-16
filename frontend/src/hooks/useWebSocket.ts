import { useEffect, useRef, useState } from 'react';
import type { TelemetryMessage } from '@/types';
import { getApiBaseUrl, getApiKey } from '@/lib/api';

function getWebSocketBaseUrl(): string {
  const apiBase = getApiBaseUrl();
  if (apiBase) {
    return apiBase.replace(/^http/, 'ws');
  }
  if (import.meta.env.VITE_WS_URL) {
    return import.meta.env.VITE_WS_URL;
  }
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${window.location.host}`;
}

export function useWebSocket(onMessage: (message: TelemetryMessage) => void) {
  const [isConnected, setIsConnected] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout>>();
  const onMessageRef = useRef(onMessage);

  // Keep the callback ref up to date
  useEffect(() => {
    onMessageRef.current = onMessage;
  }, [onMessage]);

  useEffect(() => {
    let isUnmounting = false;

    function connect() {
      if (isUnmounting) return;

      const apiKey = getApiKey();
      const url = `${getWebSocketBaseUrl()}/ws/telemetry`;
      const ws = new WebSocket(url);
      
      ws.onopen = () => {
        if (isUnmounting) {
          ws.close();
          return;
        }
        console.log('WebSocket connected');
        // Authenticate in-band without exposing credentials in URL query parameters / access logs
        if (apiKey) {
          ws.send(JSON.stringify({ type: 'auth', token: apiKey }));
        }
        setIsConnected(true);
      };

      ws.onmessage = (event) => {
        try {
          const message: TelemetryMessage = JSON.parse(event.data);
          if ((message as any).type === 'auth_ack') {
            return;
          }
          onMessageRef.current(message);
        } catch (error) {
          console.error('Failed to parse WebSocket message:', error);
        }
      };

      ws.onerror = (error) => {
        console.error('WebSocket error:', error);
      };

      ws.onclose = () => {
        if (isUnmounting) return;

        console.log('WebSocket disconnected');
        setIsConnected(false);
        
        // Attempt to reconnect after 3 seconds
        reconnectTimeoutRef.current = setTimeout(() => {
          if (!isUnmounting) {
            console.log('Attempting to reconnect...');
            connect();
          }
        }, 3000);
      };

      wsRef.current = ws;
    }

    connect();

    return () => {
      isUnmounting = true;
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
      }
      if (wsRef.current && wsRef.current.readyState === WebSocket.OPEN) {
        wsRef.current.close();
      }
    };
  }, []); // Empty dependency array - only run once

  return { isConnected };
}
