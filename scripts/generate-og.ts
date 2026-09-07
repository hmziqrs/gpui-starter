#!/usr/bin/env node
/**
 * Generate OG / social-card images for every page of the site at build time.
 *
 * Usage:
 *   bun scripts/generate-og.ts                # generate everything
 *   bun scripts/generate-og.ts <slug>         # generate cards whose out path matches <slug>
 *   bun scripts/generate-og.ts --only-missing # skip already-generated files
 *   bun scripts/generate-og.ts --out ./dist   # write under a different root
 *
 * Output (relative to public/):
 *   og-image.png            site default
 *   og/pages/<page>.png     marketing pages
 *   og/blog/<slug>.png      blog posts
 *   og/faq/<slug>.png       faq entries
 *   og/docs/<slug>.png      documentation pages
 *
 * Card design follows the warm-technical system in web/styles/globals.css.
 */

import { readFileSync, writeFileSync, mkdirSync, readdirSync, existsSync } from 'fs';
import { join, resolve, dirname } from 'path';
import satori from 'satori';
import { Resvg } from '@resvg/resvg-js';

const ROOT = resolve(import.meta.dirname, '..');
const CONTENT = join(ROOT, 'web/content');
const DEFAULT_OUT = join(ROOT, 'public');
const SITE = 'gpui-starter.freeoxide.com';

// ---------- theme (warm-technical) ----------
const T = {
  canvas: '#0d0a0c',
  panel: '#161418',
  line: '#28282b',
  rule: '#3e3e42',
  fg: '#eaeae8',
  fgMuted: '#a8a8a5',
  fgSubtle: '#6f6f72',
  accent: '#ff2e97',
};

// --- arg parsing ---
const args = process.argv.slice(2);
const outIdx = args.indexOf('--out');
const onlyMissing = args.includes('--only-missing');
const outRoot = outIdx !== -1 ? resolve(args[outIdx + 1]) : DEFAULT_OUT;
const filterArg = args.filter((a) => !a.startsWith('--') && (outIdx === -1 || args.indexOf(a) !== outIdx + 1))[0];

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

// ---------- cards ----------
interface Card {
  /** output path relative to the public root, without extension */
  out: string;
  kicker: string;
  title: string;
  description: string;
}

function collectionCards(
  dir: string,
  outPrefix: string,
  pick: (fm: Record<string, string | string[]>, slug: string) => { kicker: string; title: string; description: string },
): Card[] {
  const base = join(CONTENT, dir);
  if (!existsSync(base)) return [];
  const cards: Card[] = [];
  const walk = (rel: string) => {
    for (const entry of readdirSync(join(base, rel), { withFileTypes: true })) {
      const next = rel ? `${rel}/${entry.name}` : entry.name;
      if (entry.isDirectory()) {
        walk(next);
      } else if (entry.name.endsWith('.md') || entry.name.endsWith('.mdx')) {
        const slug = next.replace(/\.mdx?$/, '');
        const fm = parseFrontmatter(readFileSync(join(base, next), 'utf8'));
        cards.push({ out: `${outPrefix}/${slug}`, ...pick(fm, slug) });
      }
    }
  };
  walk('');
  return cards;
}

const first = (v: string | string[] | undefined, fallback: string) =>
  Array.isArray(v) ? (v[0] ?? fallback) : (v ?? fallback);

/** Static marketing pages. Titles here mirror each page's <Layout title>, trimmed for the card. */
const PAGE_CARDS: Card[] = [
  {
    out: 'og-image',
    kicker: 'Rust · GPUI',
    title: 'Ship Rust desktop apps faster',
    description:
      'A production-ready GPUI boilerplate: themes, i18n, forms, command palette, SQLite, keyring, and signed auto-update — already wired up.',
  },
  {
    out: 'og/pages/blog',
    kicker: 'Blog',
    title: 'Rust desktop apps, GPUI tutorials, and framework comparisons',
    description:
      'Tutorials, framework comparisons, and production patterns from the gpui-starter boilerplate.',
  },
  {
    out: 'og/pages/faq',
    kicker: 'FAQ',
    title: 'Answers about gpui-starter and GPUI',
    description:
      'GPUI basics, themes, i18n, forms, the command launcher, data storage, and more — answered in short.',
  },
  {
    out: 'og/pages/changelog',
    kicker: 'Changelog',
    title: 'Every feature, fix, and change',
    description: 'Release history for gpui-starter, from v0.1 to today.',
  },
  {
    out: 'og/pages/about',
    kicker: 'About',
    title: 'Why gpui-starter exists',
    description:
      'Real Rust code, everything shipping on first run, and structure taken from production GPUI apps. MIT licensed.',
  },
  {
    out: 'og/pages/privacy',
    kicker: 'Legal',
    title: 'Privacy policy',
    description:
      'How the gpui-starter website handles your data. Basic page-view stats and nothing else. The desktop app collects nothing.',
  },
  {
    out: 'og/pages/terms',
    kicker: 'Legal',
    title: 'Terms of use',
    description: 'The terms that apply to the gpui-starter website and documentation.',
  },
];

function allCards(): Card[] {
  return [
    ...PAGE_CARDS,
    ...collectionCards('blog', 'og/blog', (fm, slug) => ({
      kicker: first(fm.tags, 'GPUI'),
      title: String(fm.title ?? slug),
      description: String(fm.description ?? ''),
    })),
    ...collectionCards('faq', 'og/faq', (fm, slug) => ({
      kicker: `FAQ · ${first(fm.category, 'General')}`,
      title: String(fm.question ?? slug),
      description: String(fm.description ?? ''),
    })),
    ...collectionCards('docs', 'og/docs', (fm, slug) => ({
      kicker: 'Docs',
      title: String(fm.title ?? slug),
      description: String(fm.description ?? ''),
    })),
  ];
}

// --- fonts (bundled, so builds need no network) ---
const FONT_DIR = join(ROOT, 'node_modules/@fontsource');
function font(pkg: string, file: string) {
  const buf = readFileSync(join(FONT_DIR, pkg, 'files', file));
  return buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength) as ArrayBuffer;
}
const FONTS = [
  { name: 'Archivo', data: font('archivo', 'archivo-latin-600-normal.woff'), weight: 600 as const, style: 'normal' as const },
  { name: 'Archivo', data: font('archivo', 'archivo-latin-700-normal.woff'), weight: 700 as const, style: 'normal' as const },
  { name: 'Archivo', data: font('archivo', 'archivo-latin-800-normal.woff'), weight: 800 as const, style: 'normal' as const },
  { name: 'Martian Mono', data: font('martian-mono', 'martian-mono-latin-400-normal.woff'), weight: 400 as const, style: 'normal' as const },
];

// --- tiny JSX-free element helpers ---
type Node = { type: string; props: Record<string, unknown> };
const el = (type: string, style: Record<string, unknown>, children?: unknown): Node => ({
  type,
  props: children === undefined ? { style } : { style, children },
});

/** Long titles get a smaller size so three lines always fit. */
function titleSize(title: string) {
  if (title.length > 90) return 40;
  if (title.length > 60) return 46;
  return 54;
}

// --- image renderer ---
async function renderCard(card: Card): Promise<Buffer> {
  const svg = await satori(
    el(
      'div',
      {
        width: 1200,
        height: 630,
        display: 'flex',
        flexDirection: 'column',
        background: T.canvas,
        padding: '72px 80px',
        fontFamily: 'Archivo',
        position: 'relative',
      },
      [
        // left accent rule
        el('div', { position: 'absolute', left: 0, top: 0, bottom: 0, width: 8, background: T.accent }),
        // hairline frame echoing the site's ruled columns
        el('div', { position: 'absolute', left: 40, top: 0, bottom: 0, width: 1, background: T.line }),
        el('div', { position: 'absolute', left: 1159, top: 0, bottom: 0, width: 1, background: T.line }),

        // masthead
        el('div', { display: 'flex', alignItems: 'center', gap: 16 }, [
          el('span', { fontSize: 26, fontWeight: 700, color: T.accent, letterSpacing: '-0.01em' }, 'gpui-starter'),
          el('span', { width: 1, height: 22, background: T.rule }),
          el(
            'span',
            {
              fontFamily: 'Martian Mono',
              fontSize: 15,
              color: T.fgMuted,
              background: T.panel,
              border: `1px solid ${T.line}`,
              padding: '6px 14px',
              textTransform: 'uppercase',
              letterSpacing: '0.08em',
            },
            card.kicker,
          ),
        ]),

        el('div', { height: 1, background: T.line, marginTop: 32, marginBottom: 44 }),

        // title
        el(
          'div',
          {
            fontSize: titleSize(card.title),
            fontWeight: 800,
            color: T.fg,
            lineHeight: 1.16,
            letterSpacing: '-0.025em',
            maxWidth: 1000,
            display: '-webkit-box',
            WebkitLineClamp: 3,
            WebkitBoxOrient: 'vertical',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
          },
          card.title,
        ),

        el('div', { width: 120, height: 4, background: T.accent, marginTop: 36 }),

        // footer
        el('div', { marginTop: 'auto', display: 'flex', flexDirection: 'column' }, [
          el(
            'div',
            {
              fontSize: 21,
              fontWeight: 600,
              color: T.fgMuted,
              lineHeight: 1.45,
              maxWidth: 900,
              display: '-webkit-box',
              WebkitLineClamp: 2,
              WebkitBoxOrient: 'vertical',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            },
            card.description,
          ),
          el('div', { height: 1, background: T.line, marginTop: 28, marginBottom: 20 }),
          el('div', { display: 'flex', justifyContent: 'space-between', alignItems: 'center' }, [
            el('span', { fontFamily: 'Martian Mono', fontSize: 14, color: T.fgSubtle }, SITE),
            el('span', { fontFamily: 'Martian Mono', fontSize: 14, color: T.fgSubtle }, 'MIT · Rust · GPUI'),
          ]),
        ]),
      ],
    ) as unknown as React.ReactNode,
    { width: 1200, height: 630, fonts: FONTS },
  );

  const resvg = new Resvg(svg, { fitTo: { mode: 'width', value: 1200 } });
  return Buffer.from(resvg.render().asPng());
}

// --- main ---
async function main() {
  let cards = allCards();

  if (filterArg) cards = cards.filter((c) => c.out.includes(filterArg));
  if (cards.length === 0) {
    console.error(`No cards found${filterArg ? ` matching "${filterArg}"` : ''}.`);
    process.exit(1);
  }

  if (onlyMissing) {
    const before = cards.length;
    cards = cards.filter((c) => !existsSync(join(outRoot, `${c.out}.png`)));
    const skipped = before - cards.length;
    if (skipped > 0) console.log(`  skipping ${skipped} already-generated image(s)`);
  }

  if (cards.length === 0) {
    console.log('All OG images already up to date.');
    return;
  }

  for (const card of cards) {
    const file = join(outRoot, `${card.out}.png`);
    mkdirSync(dirname(file), { recursive: true });
    process.stdout.write(`  ${card.out}.png … `);
    writeFileSync(file, await renderCard(card));
    console.log('done');
  }

  console.log(`\n${cards.length} image(s) written to ${outRoot}`);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
