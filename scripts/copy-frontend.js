#!/usr/bin/env node
/**
 * macOS-only: copy built frontend into .app Resources.
 * On Windows/Linux Tauri already embeds frontendDist from tauri.conf.json.
 */
import { cpSync, mkdirSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { platform } from 'node:os';

if (platform() !== 'darwin') {
	console.log('Skipping copy-frontend (not required on this platform).');
	process.exit(0);
}

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = join(__dirname, '..');
const buildDir = join(rootDir, 'build');
const bundleDir = join(
	rootDir,
	'src-tauri/target/release/bundle/macos/ISearch.app/Contents/Resources'
);

if (!existsSync(buildDir)) {
	console.error('Error: build directory not found. Run "npm run build" first.');
	process.exit(1);
}

mkdirSync(bundleDir, { recursive: true });
console.log('Copying frontend files to bundle...');
cpSync(buildDir, bundleDir, { recursive: true });
console.log('Frontend files copied successfully.');
