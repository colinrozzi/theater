import React, { useState, useEffect } from 'react';
import useWebSocket from 'react-use-websocket';

const ACTOR_SERVERS = [
  { name: 'browser-ui', port: 3030 },
  { name: 'chat-state', port: 3031 },
  { name: 'llm-gateway', port: 3032 }
];

const ChainVisualizer = () => {
  const [eventsByActor, setEventsByActor] = useState({});
  const [connections, setConnections] = useState({});

  // Create WebSocket connections for each actor
  const sockets = ACTOR_SERVERS.map(server => {
    const { lastMessage } = useWebSocket(`ws://localhost:${server.port}/events/ws`, {
      onOpen: () => setConnections(prev => ({ ...prev, [server.name]: true })),
      onClose: () => setConnections(prev => ({ ...prev, [server.name]: false })),
      shouldReconnect: () => true,
      reconnectAttempts: 10,
      reconnectInterval: 3000
    });
    return { server, lastMessage };
  });

  // Fetch initial history for each actor
  useEffect(() => {
    ACTOR_SERVERS.forEach(server => {
      fetch(`http://localhost:${server.port}/events/history`)
        .then(res => res.json())
        .then(data => {
          setEventsByActor(prev => ({
            ...prev,
            [server.name]: data
          }));
        })
        .catch(console.error);
    });
  }, []);

  // Handle incoming WebSocket messages
  useEffect(() => {
    sockets.forEach(({ server, lastMessage }) => {
      if (lastMessage) {
        try {
          const event = JSON.parse(lastMessage.data);
          setEventsByActor(prev => ({
            ...prev,
            [server.name]: [...(prev[server.name] || []), event]
          }));
        } catch (err) {
          console.error('Failed to parse event:', err);
        }
      }
    });
  }, [sockets.map(s => s.lastMessage)]);

  // Find related events by chain state
  const findRelatedEvents = (event) => {
    if (!event.data?.source_chain_state) return null;
    
    const related = [];
    Object.entries(eventsByActor).forEach(([actorName, events]) => {
      events.forEach(e => {
        if (e.hash === event.data.source_chain_state) {
          related.push({ actorName, event: e });
        }
      });
    });
    return related;
  };

  return (
    <div className="min-h-screen bg-gray-100 p-8">
      <div className="max-w-full mx-auto">
        <div className="bg-white rounded-lg shadow-lg p-6 mb-6">
          <div className="flex justify-between items-center mb-6">
            <h1 className="text-2xl font-bold">Theater Chain Visualizer</h1>
            
            {/* Connection Status */}
            <div className="flex gap-4">
              {ACTOR_SERVERS.map(server => (
                <div key={server.name} className="flex items-center">
                  <span className="font-medium mr-2">{server.name}:</span>
                  <span className={`inline-block px-2 py-1 rounded text-sm ${
                    connections[server.name] 
                      ? 'bg-green-100 text-green-800' 
                      : 'bg-red-100 text-red-800'
                  }`}>
                    {connections[server.name] ? 'Connected' : 'Disconnected'}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* Actor Event Lanes */}
          <div className="grid grid-cols-3 gap-6">
            {ACTOR_SERVERS.map(server => (
              <div key={server.name} className="border rounded-lg overflow-hidden bg-white">
                <div className="bg-gray-50 px-4 py-2 border-b sticky top-0">
                  <h2 className="font-semibold">{server.name}</h2>
                  <div className="text-sm text-gray-500">
                    {(eventsByActor[server.name] || []).length} events
                  </div>
                </div>
                <div className="divide-y max-h-[700px] overflow-auto">
                  {(eventsByActor[server.name] || []).map((event, i) => {
                    const related = findRelatedEvents(event);
                    return (
                      <div 
                        key={i} 
                        className={`p-4 hover:bg-gray-50 ${
                          related?.length ? 'border-l-4 border-blue-500' : ''
                        }`}
                      >
                        <div className="font-mono text-sm text-gray-600 mb-2">
                          {new Date(event.timestamp).toLocaleString()}
                        </div>
                        <pre className="text-sm overflow-auto">
                          {JSON.stringify(event.data, null, 2)}
                        </pre>
                        {related?.length > 0 && (
                          <div className="mt-2 text-sm text-blue-600">
                            Related to events in: {related.map(r => r.actorName).join(', ')}
                          </div>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
};

export default ChainVisualizer;