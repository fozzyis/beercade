// Write .br and .gz siblings for text assets in build/, so boss-server's
// ServeDir can send them pre-compressed instead of compressing per request.
// Fonts and images are already compressed and are skipped.
import { readdir, readFile, writeFile, stat } from 'node:fs/promises';
import { join, extname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { brotliCompressSync, gzipSync, constants } from 'node:zlib';

const ROOT = fileURLToPath(new URL('../build/', import.meta.url));
const TEXT = new Set(['.js', '.css', '.html', '.svg', '.json', '.txt', '.webmanifest']);
const MIN_BYTES = 1024;

async function* walk(dir) {
	for (const entry of await readdir(dir, { withFileTypes: true })) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) yield* walk(path);
		else yield path;
	}
}

let files = 0;
let before = 0;
let after = 0;
for await (const path of walk(ROOT)) {
	if (!TEXT.has(extname(path))) continue;
	if ((await stat(path)).size < MIN_BYTES) continue;
	const data = await readFile(path);
	const br = brotliCompressSync(data, {
		params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: data.length }
	});
	await writeFile(`${path}.br`, br);
	await writeFile(`${path}.gz`, gzipSync(data, { level: 9 }));
	files += 1;
	before += data.length;
	after += br.length;
}
console.log(`precompress: ${files} files, ${(before / 1024).toFixed(0)} KiB -> ${(after / 1024).toFixed(0)} KiB (br)`);
