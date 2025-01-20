import { NextResponse } from 'next/server';
import fs from 'fs';
import path from 'path';

export async function GET() {
    try {
        // Navigate up to the project root's chain directory
        const chainDir = path.join(process.cwd(), '../../chain');
        const files = fs.readdirSync(chainDir);
        
        // Read all chain files
        const chains: Record<string, any> = {};
        for (const file of files) {
            if (file.endsWith('.json')) {
                const content = fs.readFileSync(path.join(chainDir, file), 'utf-8');
                chains[file] = JSON.parse(content);
            }
        }
        
        return NextResponse.json(chains);
    } catch (error) {
        console.error('Error reading chain files:', error);
        return NextResponse.json(
            { error: 'Failed to read chain files' },
            { status: 500 }
        );
    }
}