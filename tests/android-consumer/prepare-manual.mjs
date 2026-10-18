import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

execFileSync('npm', ['run', 'build'], {
  cwd: fileURLToPath(new URL('../../', import.meta.url)), stdio: 'inherit',
});
const built = readFileSync(new URL('../../dist-js/index.js', import.meta.url), 'utf8');
const coreImport = "import { invoke, Channel } from '@tauri-apps/api/core';";
if (!built.includes(coreImport)) {
  throw new Error('Inspect the generated Tauri import before bundling the manual app.');
}
writeFileSync(new URL('./ui/manual-api.js', import.meta.url),
  built.replace(coreImport, 'const { invoke, Channel } = window.__TAURI__.core;'));
