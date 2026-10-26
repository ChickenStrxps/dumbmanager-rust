import { readdirSync } from 'node:fs';
import { resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = resolve(fileURLToPath(new URL('../web/', import.meta.url)));
function scripts(directory) {
  return readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? scripts(path) : entry.name.endsWith('.js') ? [path] : [];
  });
}
const files = scripts(root).sort();
if (!files.length) throw new Error('No frontend scripts found');
for (const file of files) {
  const check = spawnSync(process.execPath, ['--check', file], { encoding: 'utf8' });
  if (check.error || check.status !== 0) {
    process.stderr.write(check.stderr || String(check.error));
    process.exit(check.status || 1);
  }
}
console.log(`Syntax checked ${files.length} frontend scripts.`);
