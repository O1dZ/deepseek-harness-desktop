import { cp, mkdir, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const edition = process.argv[2];
if (!['lite', 'full'].includes(edition)) {
  throw new Error('Usage: node scripts/stage-portable.mjs <lite|full>');
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const destination = path.join(root, 'release', `DeepSeek-Harness-Desktop-${edition === 'lite' ? 'Lite' : 'Full'}-x64-Portable`);
const executable = path.join(root, 'src-tauri', 'target', 'release', 'deepseek-harness-desktop.exe');

if (!destination.startsWith(path.join(root, 'release') + path.sep)) {
  throw new Error(`Refusing to stage outside the release directory: ${destination}`);
}

await rm(destination, { recursive: true, force: true });
await mkdir(destination, { recursive: true });
await cp(executable, path.join(destination, 'DeepSeek Harness Desktop.exe'));

if (edition === 'full') {
  await cp(
    path.join(root, 'src-tauri', 'resources'),
    path.join(destination, 'resources'),
    { recursive: true },
  );
}

await writeFile(
  path.join(destination, 'README.txt'),
  `DeepSeek Harness Desktop ${edition === 'lite' ? 'Lite' : 'Full'} Edition\r\n\r\n双击 “DeepSeek Harness Desktop.exe” 启动。\r\n`,
);
