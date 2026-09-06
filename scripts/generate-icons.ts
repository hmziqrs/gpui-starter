#!/usr/bin/env node
/**
 * Rasterize public/favicon.svg into the PNG icon set at build time.
 *
 * Usage:
 *   bun scripts/generate-icons.ts
 *
 * Output: public/favicon-16x16.png, favicon-32x32.png, apple-touch-icon.png,
 *         android-chrome-192x192.png, android-chrome-512x512.png
 */

import { readFileSync, writeFileSync } from 'fs';
import { join, resolve } from 'path';
import { Resvg } from '@resvg/resvg-js';

const ROOT = resolve(import.meta.dirname, '..');
const PUBLIC = join(ROOT, 'public');
const SOURCE = join(PUBLIC, 'favicon.svg');

/** apple-touch-icon is masked by iOS, so it renders on a square, corner-free plate. */
const TARGETS: { file: string; size: number; square?: boolean }[] = [
  { file: 'favicon-16x16.png', size: 16 },
  { file: 'favicon-32x32.png', size: 32 },
  { file: 'apple-touch-icon.png', size: 180, square: true },
  { file: 'android-chrome-192x192.png', size: 192 },
  { file: 'android-chrome-512x512.png', size: 512 },
];

const svg = readFileSync(SOURCE, 'utf8');
const squareSvg = svg.replace(/rx="[\d.]+"/g, 'rx="0"');

for (const { file, size, square } of TARGETS) {
  const resvg = new Resvg(square ? squareSvg : svg, { fitTo: { mode: 'width', value: size } });
  writeFileSync(join(PUBLIC, file), resvg.render().asPng());
  console.log(`  ${file} (${size}x${size})`);
}

console.log(`\n${TARGETS.length} icon(s) written to ${PUBLIC}`);
