'use client';
import { ChainViewer } from '../components/chain-viewer';

export default function Home() {
  return (
    <main className="min-h-screen bg-white shadow-sm rounded-lg">
      <div className="p-6">
        <ChainViewer />
      </div>
    </main>
  );
}
