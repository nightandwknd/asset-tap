// Runs native-probe built for wasm32-wasip1 under Node's WASI (V8). NOT a browser.
//   node run_wasi.mjs <probe.wasm> <host-workdir> <args...>   (args use /work/... paths)
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';
const [wasmPath, work, ...args] = process.argv.slice(2);
const wasi = new WASI({
  version: 'preview1',
  args: ['native-probe', ...args],
  env: Object.fromEntries(Object.entries(process.env).filter(([k]) => k.startsWith('RAYON_') || k.startsWith('SPIKE_'))),
  preopens: { '/work': work },
  returnOnExit: true,
});
const mod = await WebAssembly.compile(await readFile(wasmPath));
const inst = await WebAssembly.instantiate(mod, wasi.getImportObject());
const code = wasi.start(inst);
process.exit(code ?? 0);
