import React, { useState, useEffect } from 'react';
import useWebSocket from 'react-use-websocket';

const ACTOR_SERVERS = [
  { name: 'browser-ui', port: 3030 },
  { name: 'chat-state', port: 3031 },
  { name: 'llm-gateway', port: 3032 }
];

// Helper to get event type summary
const getEventSummary = (event) => {
  if (!event.data) return 'Unknown Event';
  
  // Handle different event types
  if ('source_actor' in event.data) return `Message from ${event.data.source_actor}`;
  if ('old_state' in event.data) return 'State Change';
  if ('input' in event.data) return 'External Input';
  if ('output' in event.data) return 'Output';
  
  return 'Unknown Event Type';
};

const EventCard = ({ event, related }) => {
  const [isExpanded, setIsExpanded] = useState(false);
  
  return (
    <div 
      className={`p-4 hover:bg-gray-50 cursor-pointer ${
        related?.length ? 'border-l-4 border-blue-500' : ''
      }`}
      onClick={() => setIsExpanded(!isExpanded)}
    >
      <div className="flex justify-between items-center">
        <div className="font-mono text-sm text-gray-600">
          {new Date(event.timestamp).toLocaleString()}
        </div>
        <div className="text-sm font-medium">
          {getEventSummary(event)}
          <span className="ml-2 text-gray-400">
            {isExpanded ? '▼' : '▶'}
          </span>
        </div>
      </div>
      
      {isExpanded && (
        <>
          <pre className="mt-4 text-sm overflow-auto bg-gray-50 p-4 rounded">
            {JSON.stringify(event.data, null, 2)}
          </pre>
          {related?.length > 0 && (
            <div className="mt-2 text-sm text-blue-600">
              Related to events in: {related.map(r => r.actorName).join(', ')}
            </div>
          )}
        </>
      )}
    </div>
  );
};

const ChainVisualizer = () => {
  const [eventsByActor, setEventsByActor] = useState({});
  const [connections, setConnections] = useState({});

  const wsOptions = {
    shouldReconnect: (closeEvent) => true,
    reconnectAttempts: 10,
    reconnectInterval: 3000,
  };

  // Create individual WebSocket connections for each actor
  const browserUISocket = useWebSocket(`ws://localhost:3030/events/ws`, {
    ...wsOptions,
    onOpen: () => setConnections(prev => ({ ...prev, 'browser-ui': true })),
    onClose: () => setConnections(prev => ({ ...prev, 'browser-ui': false })),
  });

  const chatStateSocket = useWebSocket(`ws://localhost:3031/events/ws`, {
    ...wsOptions,
    onOpen: () => setConnections(prev => ({ ...prev, 'chat-state': true })),
    onClose: () => setConnections(prev => ({ ...prev, 'chat-state': false })),
  });

  const llmGatewaySocket = useWebSocket(`ws://localhost:3032/events/ws`, {
    ...wsOptions,
    onOpen: () => setConnections(prev => ({ ...prev, 'llm-gateway': true })),
    onClose: () => setConnections(prev => ({ ...prev, 'llm-gateway': false })),
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
    const sockets = {
      'browser-ui': browserUISocket.lastMessage,
      'chat-state': chatStateSocket.lastMessage,
      'llm-gateway': llmGatewaySocket.lastMessage
    };

    Object.entries(sockets).forEach(([actorName, message]) => {
      if (message) {
        try {
          const event = JSON.parse(message.data);
          setEventsByActor(prev => ({
            ...prev,
            [actorName]: [...(prev[actorName] || []), event]
          }));
        } catch (err) {
          console.error('Failed to parse event:', err);
        }
      }
    });
  }, [browserUISocket.lastMessage, chatStateSocket.lastMessage, llmGatewaySocket.lastMessage]);

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
                <div className="bg-gray-50 px-4 py-2 border-b sticky top-0 z-10">
                  <h2 className="font-semibold">{server.name}</h2>
                  <div className="text-sm text-gray-500">
                    {(eventsByActor[server.name] || []).length} events
                  </div>
                </div>
                <div className="divide-y max-h-[700px] overflow-auto">
                  {(eventsByActor[server.name] || []).map((event, i) => (
                    <EventCard
                      key={i}
                      event={event}
                      related={findRelatedEvents(event)}
                    />
                  ))}
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