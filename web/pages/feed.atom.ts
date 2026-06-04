import { getCollection, type CollectionEntry } from 'astro:content';
import type { APIContext } from 'astro';

export async function GET(context: APIContext) {
  if (!context.site) throw new Error('astro config is missing `site`');

  const posts = await getCollection(
    'blog',
    (entry: CollectionEntry<'blog'>) => !entry.data.draft,
  );

  const sorted = posts.toSorted(
    (a: CollectionEntry<'blog'>, b: CollectionEntry<'blog'>) =>
      b.data.date.valueOf() - a.data.date.valueOf(),
  );

  const site = context.site!.toString().replace(/\/$/, '');
  const updated = sorted[0]
    ? sorted[0].data.date.toISOString()
    : new Date().toISOString();

  const entries = sorted
    .map(
      (post: CollectionEntry<'blog'>) => {
        const tags = (post.data.tags ?? [])
          .map((tag: string) => `\n    <category term="${escapeXml(tag)}"/>`)
          .join('');

        // Include the raw markdown body so feed readers that display
        // inline content can render it; the type="text/plain" attribute
        // tells them it is unprocessed Markdown, not HTML.
        const body = escapeXml(post.body ?? '');

        return `  <entry>
    <title>${escapeXml(post.data.title)}</title>
    <link href="${site}/blog/${post.id}/" rel="alternate" type="text/html"/>
    <id>${site}/blog/${post.id}/</id>
    <updated>${post.data.date.toISOString()}</updated>
    <published>${post.data.date.toISOString()}</published>
    <summary>${escapeXml(post.data.description)}</summary>${tags}
    <content type="text/plain">${body}</content>
    <author><name>hmziqrs</name><uri>https://hmziq.rs</uri></author>
  </entry>`;
      },
    )
    .join('\n');

  const atom = `<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>gpui-starter Blog</title>
  <subtitle>A production-ready Rust boilerplate for GPUI desktop apps. Build notes, tutorials, and deep dives.</subtitle>
  <link href="${site}/feed.atom" rel="self" type="application/atom+xml"/>
  <link href="${site}/" rel="alternate" type="text/html"/>
  <link href="${site}/feed.xml" rel="hub" type="application/rss+xml"/>
  <id>${site}/feed.atom</id>
  <updated>${updated}</updated>
  <author><name>hmziqrs</name><uri>https://hmziq.rs</uri></author>
  <generator uri="https://astro.build/">Astro</generator>
  <icon>${site}/favicon.svg</icon>
  <rights>Copyright ${new Date().getFullYear()} hmziqrs. All rights reserved.</rights>
${entries}
</feed>`;

  return new Response(atom, {
    headers: {
      'Content-Type': 'application/atom+xml; charset=utf-8',
      'Cache-Control': 'max-age=3600',
    },
  });
}

function escapeXml(str: string): string {
  return str
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&apos;');
}
