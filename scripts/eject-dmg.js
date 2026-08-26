import { execSync } from 'node:child_process';
import { existsSync, readdirSync, unlinkSync } from 'node:fs';
import { join } from 'node:path';

const macosDir = join(process.cwd(), 'src-tauri/target/release/bundle/macos');

function detach(target) {
	try {
		execSync(`hdiutil detach ${JSON.stringify(target)} -force`, { stdio: 'ignore' });
	} catch {
		// Volume may already be ejected.
	}
}

if (existsSync(macosDir)) {
	for (const name of readdirSync(macosDir)) {
		if (!name.startsWith('rw.') || !name.endsWith('.dmg')) continue;
		const image = join(macosDir, name);
		detach(image);
		try {
			unlinkSync(image);
		} catch {
			// Ignore if the image is already gone.
		}
	}
}

for (const volume of ['/Volumes/ISearch']) {
	if (existsSync(volume)) detach(volume);
}
