import React, { useState, useEffect } from 'react';

type Event = {
  type_: string;
  parent: number | null;
  data: number[];
};

type MetaEvent = {
  hash: number;
  event: Event;
};

type Chain = {
  events: MetaEvent[];
};

type DecodedResult = {
  value: any;
  type: 'string' | 'json' | 'bytes' | 'array';
  depth: number;
};

export const ChainViewer: React.FC = () => {
  const [chains, setChains] = useState<Record<string, Chain>>({});
  const [selectedFile, setSelectedFile] = useState<string>('');
  const [error, setError] = useState<string | null>(null);
  const [showRaw, setShowRaw] = useState<Record<string, boolean>>({});
  
  // Helper to deeply decode byte arrays with tracking of nesting depth
  const decodeBytes = (bytes: number[], depth = 0): DecodedResult => {
    try {
      // Convert bytes to string
      const str = new TextDecoder().decode(new Uint8Array(bytes));
      
      try {
        // Try to parse as JSON
        const parsed = JSON.parse(str);
        
        // If we got an array, recursively decode its elements
        if (Array.isArray(parsed)) {
          const decodedArray = parsed.map(item => {
            if (Array.isArray(item) && item.every(num => typeof num === 'number')) {
              return decodeBytes(item, depth + 1);
            }
            return { value: item, type: typeof item as 'string', depth: depth + 1 };
          });
          
          // If all items are strings and we only have one, return it directly
          if (decodedArray.length === 1 && decodedArray[0].type === 'string') {
            return { value: decodedArray[0].value, type: 'string', depth };
          }
          
          return { value: decodedArray, type: 'array', depth };
        }
        
        // If it's a regular JSON object/value
        return { value: parsed, type: 'json', depth };
      } catch {
        // If it's not valid JSON but we could decode it as a string
        return { value: str, type: 'string', depth };
      }
    } catch {
      // If we couldn't decode at all
      return { value: bytes, type: 'bytes', depth };
    }
  };
  
  // Format the decoded result for display
  const formatDecodedResult = (result: DecodedResult): string => {
    if (result.type === 'array') {
      const formatted = result.value.map((item: DecodedResult) => 
        item.type === 'bytes' ? `[${item.value.join(', ')}]` : formatDecodedResult(item)
      );
      return JSON.stringify(formatted, null, 2);
    }
    
    if (result.type === 'json') {
      return JSON.stringify(result.value, null, 2);
    }
    
    if (result.type === 'bytes') {
      return `[${result.value.join(', ')}]`;
    }
    
    return result.value.toString();
  };

  // Helper to format event data
  const formatEventData = (event: Event): { 
    formatted: string; 
    decoded: DecodedResult;
    raw: string;
  } => {
    const decoded = decodeBytes(event.data);
    return {
      formatted: formatDecodedResult(decoded),
      decoded,
      raw: `[${event.data.join(', ')}]`
    };
  };

  useEffect(() => {
    const loadChains = async () => {
      try {
        const response = await fetch('/api/chains');
        if (!response.ok) throw new Error('Failed to load chain data');
        const data = await response.json();
        
        setChains(data);
        if (!selectedFile && Object.keys(data).length > 0) {
          setSelectedFile(Object.keys(data)[0]);
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : 'Failed to load chain data');
        console.error('Error loading chains:', e);
      }
    };

    loadChains();
  }, []);

  const toggleRawView = (hash: number) => {
    setShowRaw(prev => ({
      ...prev,
      [hash]: !prev[hash]
    }));
  };

  if (error) {
    return (
      <div className="p-4">
        <div className="text-red-600 bg-red-50 p-4 rounded-lg border border-red-200">
          Error: {error}
        </div>
      </div>
    );
  }

  const currentChain = selectedFile ? chains[selectedFile] : null;

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h1 className="text-2xl font-bold">Theater Chain Viewer</h1>
        <select 
          value={selectedFile}
          onChange={(e) => setSelectedFile(e.target.value)}
          className="px-3 py-2 border rounded-md"
        >
          {Object.keys(chains).map(file => (
            <option key={file} value={file}>{file.replace('.json', '')}</option>
          ))}
        </select>
      </div>

      {currentChain && (
        <div className="space-y-4">
          {currentChain.events.map((meta, index) => {
            const { formatted, decoded, raw } = formatEventData(meta.event);
            const isRawView = showRaw[meta.hash];
            
            return (
              <div key={meta.hash} className="border rounded-lg p-4 bg-gray-50">
                <div className="flex justify-between text-sm text-gray-500 mb-2">
                  <span>Event #{index + 1}</span>
                  <span className="font-mono">Hash: {meta.hash}</span>
                </div>
                <div className="grid grid-cols-2 gap-4 mb-4">
                  <div>
                    <div className="font-semibold text-gray-600">Type</div>
                    <div className="text-blue-600 font-medium">{meta.event.type_}</div>
                  </div>
                  <div>
                    <div className="font-semibold text-gray-600">Parent</div>
                    <div className="font-mono text-sm">{meta.event.parent || 'None'}</div>
                  </div>
                </div>
                <div>
                  <div className="flex items-center justify-between mb-1">
                    <div className="font-semibold text-gray-600">Data</div>
                    {decoded.depth > 0 && (
                      <button
                        onClick={() => toggleRawView(meta.hash)}
                        className="text-sm text-blue-500 hover:text-blue-600"
                      >
                        {isRawView ? 'Show Decoded' : 'Show Raw'}
                      </button>
                    )}
                  </div>
                  <pre 
                    className={`p-3 rounded-md border font-mono text-sm overflow-x-auto ${
                      isRawView ? 'bg-gray-100' : 'bg-white'
                    }`}
                  >
                    {isRawView ? raw : formatted}
                  </pre>
                  {decoded.depth > 0 && (
                    <div className="mt-1 text-xs text-gray-500">
                      Nested Depth: {decoded.depth}
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
};