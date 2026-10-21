#!/usr/bin/env node
// Exercise the actual wasm32-unknown-unknown module through generated Node bindings.
const fs = require('node:fs');
const path = require('node:path');
const [modulePath, outDir, mode = 'both'] = process.argv.slice(2);
if (!modulePath || !outDir || !['native', 'both'].includes(mode)) {
  console.error('usage: node smoke_wasm.cjs <bindgen/eval.js> <new-output-dir> [native|both]');
  process.exit(2);
}
const api = require(path.resolve(modulePath));
if (fs.existsSync(outDir)) throw new Error('select a new output directory');
fs.mkdirSync(outDir, { recursive: true });
const results = [];
for (const writer of mode === 'both' ? ['native', 'terrio'] : ['native']) {
  for (const kind of ['rgb', 'float-nodata', 'uint16', 'palette', 'classes', 'gray-alpha', 'rgba', 'odd', 'bigtiff']) {
    const [width, height] = kind === 'rgb' ? [1024, 768] : [1025, 769];
    const file = path.join(outDir, `${writer}-${kind}.tif`);
    try {
      const bytes = api.export_case(writer, kind, width, height, kind === 'bigtiff');
      fs.writeFileSync(file, bytes, { flag: 'wx' });
      results.push({writer, case: kind, width, height, bytes: bytes.length, file, status: 'exported'});
    } catch (e) {
      results.push({writer, case: kind, status: 'error', error: String(e)});
    }
  }
}
for (const args of [['native', 'rgb', 0, 1, false], ['native', 'rgb', 0xffffffff, 0xffffffff, false], ['native', 'unknown', 1, 1, false]]) {
  let rejected = false;
  try { api.export_case(...args); } catch (_) { rejected = true; }
  results.push({hostile: args.slice(1), rejected});
}
fs.writeFileSync(path.join(outDir, 'wasm-smoke.json'), JSON.stringify(results, null, 2) + '\n', { flag: 'wx' });
console.log(JSON.stringify(results, null, 2));
if (results.some(x => x.status === 'error' || x.rejected === false)) process.exitCode = 1;
