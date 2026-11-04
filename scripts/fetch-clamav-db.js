#!/usr/bin/env node
/**
 * Ensure the ClamAV virus database exists in <project>/clamav_db before bundling.
 *
 * Prefers the official `freshclam` updater; falls back to downloading the .cvd
 * files directly from database.clamav.net. Signature databases are
 * platform-independent, so the same files work for every bundle target.
 *
 * Usage: node scripts/fetch-clamav-db.js [--optional]
 *   --optional  warn instead of failing when the download fails (used by `tauri dev`)
 */
import { spawnSync } from 'node:child_process';
import { createWriteStream, existsSync, mkdirSync, openSync, readSync, closeSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir, platform, arch } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { Readable } from 'node:stream';
import { pipeline } from 'node:stream/promises';
import { fileURLToPath } from 'node:url';

const MIRROR = 'https://database.clamav.net';
const DATABASES = ['main', 'daily', 'bytecode'];

const optional = process.argv.includes('--optional');
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const dbDir = join(root, 'clamav_db');

function hasDb(name) {
	return existsSync(join(dbDir, `${name}.cvd`)) || existsSync(join(dbDir, `${name}.cld`));
}

function isComplete() {
	return DATABASES.every(hasDb);
}

function tryFreshclam() {
	const probe = spawnSync('freshclam', ['--version'], { encoding: 'utf8' });
	if (probe.error || probe.status !== 0) {
		console.log('[clamav-db] freshclam not found, using direct download');
		return false;
	}
	console.log(`[clamav-db] Using ${probe.stdout.trim()}`);

	const conf = join(tmpdir(), `isearch-freshclam-${process.pid}.conf`);
	writeFileSync(conf, `DatabaseDirectory ${dbDir}\nDatabaseMirror database.clamav.net\nForeground yes\n`);
	try {
		const res = spawnSync('freshclam', [`--config-file=${conf}`, '--stdout'], { stdio: 'inherit' });
		return res.status === 0;
	} finally {
		rmSync(conf, { force: true });
	}
}

function looksLikeCvd(file) {
	const fd = openSync(file, 'r');
	try {
		const buf = Buffer.alloc(11);
		readSync(fd, buf, 0, 11, 0);
		return buf.toString('latin1') === 'ClamAV-VDB:';
	} finally {
		closeSync(fd);
	}
}

async function download(name) {
	const dest = join(dbDir, `${name}.cvd`);
	const tmp = `${dest}.part`;
	const url = `${MIRROR}/${name}.cvd`;
	console.log(`[clamav-db] Downloading ${url}`);

	// The ClamAV CDN rejects requests that don't identify as a ClamAV client.
	const res = await fetch(url, {
		headers: { 'User-Agent': `ClamAV/1.4.0 (OS: ${platform()}, ARCH: ${arch()})` }
	});
	if (!res.ok || !res.body) {
		throw new Error(`${url} -> HTTP ${res.status}`);
	}
	await pipeline(Readable.fromWeb(res.body), createWriteStream(tmp));

	if (!looksLikeCvd(tmp)) {
		rmSync(tmp, { force: true });
		throw new Error(`${url} did not return a valid CVD file (likely blocked or rate-limited)`);
	}
	renameSync(tmp, dest);
}

async function main() {
	if (isComplete()) {
		console.log(`[clamav-db] Database present at ${dbDir}`);
		return;
	}

	mkdirSync(dbDir, { recursive: true });
	console.log(`[clamav-db] Database missing, fetching into ${dbDir}`);

	if (!tryFreshclam() || !isComplete()) {
		for (const name of DATABASES) {
			if (!hasDb(name)) {
				await download(name);
			}
		}
	}

	if (!isComplete()) {
		throw new Error('database is still incomplete after download');
	}
	console.log('[clamav-db] Database ready');
}

main().catch((err) => {
	if (optional) {
		console.warn(`[clamav-db] Warning: ${err.message}. Continuing without a bundled database.`);
		mkdirSync(dbDir, { recursive: true });
		return;
	}
	console.error(`[clamav-db] Error: ${err.message}`);
	console.error('[clamav-db] Install ClamAV (freshclam) or retry later; see https://www.clamav.net/downloads');
	process.exit(1);
});
