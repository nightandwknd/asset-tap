// Drives wasm-probe (wasm32-unknown-unknown, the browser target) under Node/V8. NOT a browser.
//   node run_wasm_probe.cjs <pkg-dir> <baked.glb>...
const fs = require('fs'), path = require('path');
const [pkg, ...glbs] = process.argv.slice(2);
const p = require(path.resolve(pkg, 'wasm_probe.js'));
console.log('node', process.version, 'v8', process.versions.v8);
console.log('rayon_probe:', p.rayon_probe());
console.log('path_api_probe(/x.glb):', p.path_api_probe('/x.glb'));
for (const f of glbs) {
  const b = fs.readFileSync(f);
  for (let rep = 1; rep <= 3; rep++) console.log(path.basename(f), b.length + ' B', 'rep' + rep, p.play_probe(new Uint8Array(b), 1000));
}
