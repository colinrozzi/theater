# Theater Chain Viewer

A tool for visualizing and debugging Theater actor chains.

## Features
- View chain data for each actor in a human-readable format
- Automatic conversion of byte arrays to strings/JSON
- Navigate between different actor chains
- Visualize parent-child relationships between events

## Setup
```bash
npm install
npm run dev
```

## Usage
1. Make sure your Theater instance is saving chains to the `chain` directory
2. Start the chain viewer
3. Navigate to http://localhost:3000
4. Select different actor chains from the dropdown menu

## Development
- Built with Next.js and TailwindCSS
- TypeScript for type safety
- React for UI components