import { spawnSync } from 'node:child_process';
import { createWriteStream } from 'node:fs';
import { access, cp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { Readable } from 'node:stream';
import { finished } from 'node:stream/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import extract from 'extract-zip';

const NODE_VERSION = '24.18.0';
const DSH_VERSION = '0.1.0-rc.6';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = path.join(root, 'runtime', 'full');
const destination = path.join(root, 'src-tauri', 'resources', 'full-runtime');
const downloads = path.join(root, '.cache', 'downloads');
const archive = path.join(downloads, `node-v${NODE_VERSION}-win-x64.zip`);
const extracted = path.join(downloads, `node-v${NODE_VERSION}-win-x64`);

if (!destination.startsWith(path.join(root, 'src-tauri', 'resources') + path.sep)) {
  throw new Error(`Refusing to prepare runtime outside the project: ${destination}`);
}

await rm(destination, { recursive: true, force: true });
await mkdir(path.join(destination, 'app'), { recursive: true });
await mkdir(downloads, { recursive: true });

try {
  await access(archive);
} catch {
  const url = `https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-win-x64.zip`;
  const response = await fetch(url);
  if (!response.ok || !response.body) {
    throw new Error(`Unable to download portable Node.js (${response.status}): ${url}`);
  }
  await finished(Readable.fromWeb(response.body).pipe(createWriteStream(archive)));
}

try {
  await access(path.join(extracted, 'node.exe'));
} catch {
  await rm(extracted, { recursive: true, force: true });
  await extract(archive, { dir: downloads });
}

await cp(path.join(extracted, 'node.exe'), path.join(destination, 'node.exe'));
await cp(path.join(source, 'package.json'), path.join(destination, 'app', 'package.json'));

const lockPath = path.join(source, 'package-lock.json');
try {
  await access(lockPath);
  await cp(lockPath, path.join(destination, 'app', 'package-lock.json'));
} catch {
  // npm install below creates a deterministic lock for local development;
  // CI requires the checked-in lock once it exists.
}

const npmCli = path.join(path.dirname(process.execPath), 'node_modules', 'npm', 'bin', 'npm-cli.js');
const install = spawnSync(
  process.execPath,
  [npmCli, 'ci', '--omit=dev', '--no-audit', '--no-fund', '--cache', path.join(root, '.cache', 'npm-full')],
  { cwd: path.join(destination, 'app'), stdio: 'inherit', shell: false },
);
if (install.status !== 0) {
  throw new Error(`Full Runtime npm install failed: ${install.error || `exit code ${install.status}`}`);
}

const manifest = JSON.parse(await readFile(path.join(destination, 'app', 'node_modules', '@deepseek-ai', 'dsh', 'package.json'), 'utf8'));
if (manifest.version !== DSH_VERSION) {
  throw new Error(`Expected dsh ${DSH_VERSION}, installed ${manifest.version}`);
}

await writeFile(
  path.join(destination, 'runtime.json'),
  `${JSON.stringify({ node: NODE_VERSION, dsh: DSH_VERSION, architecture: 'x64' }, null, 2)}\n`,
);

process.stdout.write(`Prepared Full Runtime: Node ${NODE_VERSION}, dsh ${DSH_VERSION}\n`);
