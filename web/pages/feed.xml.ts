import rss from '@astrojs/rss';
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

  return rss({
    title: 'gpui-starter Blog',
    description:
      'A production-ready Rust boilerplate for GPUI desktop apps. Build notes, tutorials, and deep dives.',
    site: context.site,
    items: sorted.map((post: CollectionEntry<'blog'>) => ({
      title: post.data.title,
      pubDate: post.data.date,
      description: post.data.description,
      link: `/blog/${post.id}/`,
      categories: post.data.tags,
    })),
    customData: '<language>en-us</language>',
  });
}
