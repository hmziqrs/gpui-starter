#!/usr/bin/env node
/**
 * Generate OG images for blog posts on the fly.
 *
 * Usage:
 *   bun scripts/generate-og.ts                       # generate all posts
 *   bun scripts/generate-og.ts <slug>                # generate one post
 *   bun scripts/generate-og.ts --out ./dist/og       # custom output dir
 *   bun scripts/generate-og.ts --only-missing        # skip already-generated
 *
 * Output: public/og/blog/<slug>.png (or --out dir)
 */

import { readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync } from 'fs';
import { join, resolve, basename } from 'path';
import satori from 'satori';
import { Resvg } from '@resvg/resvg-js';

const ROOT = resolve(import.meta.dirname, '..');
const BLOG_DIR = join(ROOT, 'web/content/blog');
const DEFAULT_OUT = join(ROOT, 'public/og/blog');

// --- arg parsing ---
const args = process.argv.slice(2);
const outIdx = args.indexOf('--out');
const onlyMissing = args.includes('--only-missing');
const outDir = outIdx !== -1 ? resolve(args[outIdx + 1]) : DEFAULT_OUT;
const slugArg = args.filter((a) => !a.startsWith('--') && (outIdx === -1 || args.indexOf(a) !== outIdx + 1))[0];

// --- frontmatter parser (no extra dep) ---
function parseFrontmatter(src: string): Record<string, string | string[]> {
  const match = src.match(/^---\r?\n([\s\S]*?)\r?\n---/);
  if (!match) return {};
  const result: Record<string, string | string[]> = {};
  for (const line of match[1].split('\n')) {
    const colon = line.indexOf(':');
    if (colon === -1) continue;
    const key = line.slice(0, colon).trim();
    const raw = line.slice(colon + 1).trim().replace(/^"|"$/g, '');
    if (raw.startsWith('[')) {
      result[key] = raw
        .slice(1, -1)
        .split(',')
        .map((s) => s.trim().replace(/^['"]|['"]$/g, ''));
    } else {
      result[key] = raw;
    }
  }
  return result;
}

function getPostMeta(file: string): { slug: string; title: string; description: string; tag: string } {
  const src = readFileSync(file, 'utf8');
  const fm = parseFrontmatter(src);
  const slug = basename(file, '.md');
  const title = String(fm.title ?? slug);
  const description = String(fm.description ?? '');
  const tags = fm.tags;
  const tag = Array.isArray(tags) ? tags[0] : String(tags ?? 'GPUI');
  return { slug, title, description, tag };
}

// --- font fetch (cached in memory for batch runs) ---
let fontCache: { w700: ArrayBuffer; w800: ArrayBuffer } | null = null;
async function getFonts() {
  if (fontCache) return fontCache;
  const [w700, w800] = await Promise.all([
    fetch('https://cdn.jsdelivr.net/npm/@fontsource/inter@5/files/inter-latin-700-normal.woff').then((r) => r.arrayBuffer()),
    fetch('https://cdn.jsdelivr.net/npm/@fontsource/inter@5/files/inter-latin-800-normal.woff').then((r) => r.arrayBuffer()),
  ]);
  fontCache = { w700, w800 };
  return fontCache;
}

// --- image renderer ---
async function renderOG(title: string, description: string, tag: string): Promise<Buffer> {
  const { w700, w800 } = await getFonts();

  const svg = await satori(
    {
      type: 'div',
      props: {
        style: {
          width: 1200,
          height: 630,
          display: 'flex',
          flexDirection: 'column',
          background: 'linear-gradient(135deg, #06060a 0%, #1a1a2e 100%)',
          padding: '80px',
          position: 'relative',
        },
        children: [
          {
            type: 'div',
            props: {
              style: {
                position: 'absolute',
                left: 0,
                top: 0,
                bottom: 0,
                width: 6,
                background: 'linear-gradient(180deg, #fbbf24, #f59e0b)',
              },
            },
          },
          {
            type: 'div',
            props: {
              style: { display: 'flex', alignItems: 'center', gap: 16, marginBottom: 24 },
              children: [
                {
                  type: 'span',
                  props: {
                    style: { fontSize: 28, fontWeight: 600, color: '#f59e0b' },
                    children: 'gpui-starter',
                  },
                },
                {
                  type: 'span',
                  props: { style: { fontSize: 20, color: '#4b4b5a' }, children: '|' },
                },
                {
                  type: 'span',
                  props: {
                    style: {
                      fontSize: 18,
                      color: '#818194',
                      background: 'rgba(245, 158, 11, 0.1)',
                      padding: '4px 12px',
                      borderRadius: 6,
                      border: '1px solid rgba(245, 158, 11, 0.15)',
                    },
                    children: tag,
                  },
                },
              ],
            },
          },
          {
            type: 'div',
            props: {
              style: {
                width: 120,
                height: 3,
                background: 'linear-gradient(90deg, #fbbf24, #f59e0b)',
                borderRadius: 2,
                marginBottom: 40,
              },
            },
          },
          {
            type: 'div',
            props: {
              style: {
                fontSize: 52,
                fontWeight: 800,
                color: '#e7e7ed',
                lineHeight: 1.2,
                letterSpacing: '-0.02em',
                maxWidth: 1040,
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                display: '-webkit-box',
                WebkitLineClamp: 3,
                WebkitBoxOrient: 'vertical',
              },
              children: title,
            },
          },
          {
            type: 'div',
            props: {
              style: {
                marginTop: 'auto',
                display: 'flex',
                justifyContent: 'space-between',
                alignItems: 'flex-end',
              },
              children: [
                {
                  type: 'div',
                  props: {
                    style: {
                      fontSize: 20,
                      color: '#a8a8b8',
                      maxWidth: 800,
                      overflow: 'hidden',
                      textOverflow: 'ellipsis',
                      display: '-webkit-box',
                      WebkitLineClamp: 2,
                      WebkitBoxOrient: 'vertical',
                      lineHeight: 1.4,
                    },
                    children: description,
                  },
                },
                {
                  type: 'span',
                  props: {
                    style: { fontSize: 14, fontFamily: 'monospace', color: '#4b4b5a' },
                    children: 'gpui-starter.hmziq.xyz',
                  },
                },
              ],
            },
          },
        ],
      },
    },
    {
      width: 1200,
      height: 630,
      fonts: [
        { name: 'Inter', data: w700, weight: 700, style: 'normal' },
        { name: 'Inter', data: w800, weight: 800, style: 'normal' },
      ],
    },
  );

  const resvg = new Resvg(svg, { fitTo: { mode: 'width', value: 1200 } });
  return Buffer.from(resvg.render().asPng());
}

// --- main ---
async function main() {
  mkdirSync(outDir, { recursive: true });

  let files = slugArg
    ? [join(BLOG_DIR, `${slugArg}.md`)]
    : readdirSync(BLOG_DIR)
        .filter((f) => f.endsWith('.md'))
        .map((f) => join(BLOG_DIR, f));

  if (files.length === 0) {
    console.error(`No blog posts found${slugArg ? ` for slug "${slugArg}"` : ''}.`);
    process.exit(1);
  }

  if (onlyMissing) {
    const before = files.length;
    files = files.filter((f) => {
      const slug = basename(f, '.md');
      return !existsSync(join(outDir, `${slug}.png`));
    });
    const skipped = before - files.length;
    if (skipped > 0) console.log(`  skipping ${skipped} already-generated image(s)`);
  }

  if (files.length === 0) {
    console.log('All OG images already up to date.');
    return;
  }

  for (const file of files) {
    const { slug, title, description, tag } = getPostMeta(file);
    process.stdout.write(`  generating ${slug}.png … `);
    const png = await renderOG(title, description, tag);
    writeFileSync(join(outDir, `${slug}.png`), png);
    console.log('done');
  }

  console.log(`\n${files.length} image(s) written to ${outDir}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
