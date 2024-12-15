import React from 'react';
import ReactDOM from 'react-dom/client';
import './index.css';
import ChainVisualizer from './ChainVisualizer';

const root = ReactDOM.createRoot(document.getElementById('root'));
root.render(
  <React.StrictMode>
    <ChainVisualizer />
  </React.StrictMode>
);