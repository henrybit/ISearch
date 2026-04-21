import { cpSync, mkdirSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = join(__dirname, '..');
const buildDir = join(rootDir, 'build');
const bundleDir = join(rootDir, 'src-tauri/target/release/bundle/macos/ISearch.app/Contents/Resources');

// Ensure build directory exists
if (!existsSync(buildDir)) {
  console.error('Error: build directory not found. Run "npm run build" first.');
  process.exit(1);
}

// Ensure Resources directory exists
mkdirSync(bundleDir, { recursive: true });

// Copy frontend build files to bundle Resources
console.log('Copying frontend files to bundle...');
cpSync(buildDir, bundleDir, { recursive: true });
console.log('Frontend files copied successfully.');
