#!/usr/bin/env node
/**
 * Cross-platform Tauri build.
 * Windows only accepts: msi, nsis
 * macOS accepts: app, dmg
 * Linux accepts: deb, rpm, appimage
 */
import { execSync } from 'node:child_process';
import { platform } from 'node:os';

const mode = process.argv[2] || 'default'; // default | full | dmg
const os = platform();

function run(cmd) {
	console.log(`> ${cmd}`);
	execSync(cmd, { stdio: 'inherit', env: { ...process.env, CI: process.env.CI || 'true' } });
}

let bundles;
let extraBefore = null;

if (os === 'win32') {
	bundles = mode === 'full' ? null : 'nsis';
} else if (os === 'darwin') {
	if (mode === 'dmg' || mode === 'macos') {
		extraBefore = 'node scripts/eject-dmg.js';
		bundles = 'app,dmg';
	} else if (mode === 'full') {
		bundles = null;
	} else {
		bundles = 'app';
	}
} else {
	// linux
	bundles = mode === 'full' ? null : 'appimage';
}

if (extraBefore) {
	run(extraBefore);
}

const bundleArg = bundles ? `--bundles ${bundles}` : '';
run(`npx @tauri-apps/cli build ${bundleArg}`.trim());
run('node scripts/copy-frontend.js');
run('node scripts/print-tauri-artifacts.js');
