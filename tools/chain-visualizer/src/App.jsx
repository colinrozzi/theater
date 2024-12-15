import React, { useState, useEffect } from 'react';
import { LineChart, Line, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts';

const EventNode = ({ event, onClick }) => (
  <div 
    className="p-4 bg-white shadow rounded-lg cursor-pointer hover:shadow-lg transition-shadow"
    onClick={() => onClick(event)}
  >
    <h3 className="font-bold text-lg">{event.event.type}</h3>
    <pre className="mt-2 text-sm overflow-x-auto">
      {JSON.stringify(event.event.data, null, 2)}
    </pre>
  </div>
);

const EventChain = ({ events }) => {
  const [selectedEvent, setSelectedEvent] = useState(null);

  return (
    <div className="p-4">
      <h2 className="text-2xl font-bold mb-4">Event Chain</h2>
      
      <div className="grid grid-cols-1 gap-4">
        {events.map((event, index) => (
          <div key={index} className="relative">
            {index > 0 && (
              <div className="absolute h-full w-0.5 bg-gray-200 left-1/2 -top-4 -z-10" />
            )}
            <EventNode 
              event={event}
              onClick={setSelectedEvent}
            />
          </div>
        ))}
      </div>

      {selectedEvent && (
        <div className="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center p-4">
          <div className="bg-white rounded-lg p-6 max-w-2xl w-full max-h-[80vh] overflow-y-auto">
            <h3 className="text-xl font-bold mb-4">Event Details</h3>
            <pre className="whitespace-pre-wrap">
              {JSON.stringify(selectedEvent, null, 2)}
            </pre>
            <button
              className="mt-4 px-4 py-2 bg-blue-500 text-white rounded hover:bg-blue-600"
              onClick={() => setSelectedEvent(null)}
            >
              Close
            </button>
          </div>
        </div>
      )}
    </div>
  );
};

const App = () => {
  const [events, setEvents] = useState([]);

  // TODO: Replace with real data fetching
  useEffect(() => {
    // Example data
    setEvents([
      {
        event: {
          type: "state_change",
          data: { new_state: "value1" }
        },
        parent: null
      },
      {
        event: {
          type: "message_received",
          data: { content: "Hello World" }
        },
        parent: "hash1"
      },
      // Add more example events here
    ]);
  }, []);

  return (
    <div className="min-h-screen bg-gray-50">
      <header className="bg-white shadow">
        <div className="max-w-7xl mx-auto py-6 px-4">
          <h1 className="text-3xl font-bold text-gray-900">
            Theater Chain Visualizer
          </h1>
        </div>
      </header>

      <main className="max-w-7xl mx-auto py-6 px-4">
        <EventChain events={events} />
      </main>
    </div>
  );
};

export default App;