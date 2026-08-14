import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';

const readJson = async (path) => JSON.parse(await readFile(path, 'utf8'));

test('Full Runtime and desktop agree on the pinned dsh version', async () => {
  const runtime = await readJson(new URL('../runtime/full/package.json', import.meta.url));
  const rust = await readFile(new URL('../src-tauri/src/runtime.rs', import.meta.url), 'utf8');
  assert.equal(runtime.dependencies['@deepseek-ai/dsh'], '0.1.0-rc.6');
  assert.match(rust, /DSH_VERSION: &str = "0\.1\.0-rc\.6"/);
});

test('Both release editions use the same application identifier', async () => {
  const base = await readJson(new URL('../src-tauri/tauri.conf.json', import.meta.url));
  const lite = await readJson(new URL('../src-tauri/tauri.lite.conf.json', import.meta.url));
  const full = await readJson(new URL('../src-tauri/tauri.full.conf.json', import.meta.url));
  assert.equal(base.identifier, 'io.github.deepseek-harness-desktop');
  assert.equal(lite.identifier, undefined);
  assert.equal(full.identifier, undefined);
});

test('Remote Harness pages receive no Tauri capability', async () => {
  const capability = await readJson(new URL('../src-tauri/capabilities/main.json', import.meta.url));
  assert.equal(capability.local, true);
  assert.equal(capability.remote, undefined);
});
