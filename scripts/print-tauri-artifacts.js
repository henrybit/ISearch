#!/usr/bin/env node
import { existsSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { platform } from 'node:os';

const root = join(process.cwd(), 'src-tauri', 'target', 'release', 'bundle');

const dirsByOs = {
	win32: ['nsis', 'msi'],
	darwin: ['dmg', 'macos'],
	linux: ['appimage', 'deb', 'rpm'],
};

const dirs = dirsByOs[platform()] || Object.values(dirsByOs).flat();

console.log('\nBuild artifacts:');
let found = false;

for (const dir of dirs) {
	const full = join(root, dir);
	if (!existsSync(full)) continue;
	for (const name of readdirSync(full)) {
		const path = join(full, name);
		try {
			const size = statSync(path).size;
			const mb = (size / (1024 * 1024)).toFixed(1);
			console.log(`  ${path} (${mb} MB)`);
			found = true;
		} catch {
			console.log(`  ${path}`);
			found = true;
		}
	}
}

if (!found) {
	console.log(`  (none found under ${root})`);
}
