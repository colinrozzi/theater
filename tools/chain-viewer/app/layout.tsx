import './globals.css'
import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Theater Chain Viewer',
  description: 'Visualize and debug Theater actor chains',
}

export default function RootLayout({
  children,
}: {
  children: React.ReactNode
}) {
  return (
    <html lang="en">
      <body className="min-h-screen bg-gray-50">
        <div className="max-w-7xl mx-auto py-6 px-4 sm:px-6 lg:px-8">
          {children}
        </div>
      </body>
    </html>
  )
}