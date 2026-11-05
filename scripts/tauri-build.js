#!/usr/bin/env node
/**
 * Cross-platform Tauri build.
 * Windows only accepts: msi, nsis
 * macOS accepts: app, dmg
 * Linux accepts: deb, rpm, appimage
 */
import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { platform } from 'node:os';

const mode = process.argv[2] || 'default'; // default | full | dmg
const os = platform();
const env = { ...process.env, CI: process.env.CI || 'true' };

function run(cmd) {
	console.log(`> ${cmd}`);
	execSync(cmd, { stdio: 'inherit', env });
}

let bundles;
let extraBefore = null;
let extraArgs = '';

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

	// Ship libclamav and its Homebrew deps inside the .app instead of requiring Homebrew at runtime.
	run('bash scripts/macos-vendor-dylibs.sh');
	for (const line of readFileSync('src-tauri/macos-libs/env', 'utf8').split('\n')) {
		const i = line.indexOf('=');
		if (i > 0) env[line.slice(0, i)] = line.slice(i + 1);
	}
	extraArgs = '--config src-tauri/tauri.macos-libs.conf.json';
} else {
	// linux
	bundles = mode === 'full' ? null : 'appimage';
}

if (extraBefore) {
	run(extraBefore);
}

const bundleArg = bundles ? `--bundles ${bundles}` : '';
run(`npx @tauri-apps/cli build ${bundleArg} ${extraArgs}`.trim());
run('node scripts/copy-frontend.js');
run('node scripts/print-tauri-artifacts.js');
