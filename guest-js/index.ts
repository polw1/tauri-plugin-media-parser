import { invoke } from '@tauri-apps/api/core';

/**
 * Says hello from the Media Parser plugin
 * @param name - The name to greet
 * @returns A greeting message
 */
export async function hello(name: string): Promise<string> {
   return await invoke<string>('plugin:media-parser|hello', { name });
}

