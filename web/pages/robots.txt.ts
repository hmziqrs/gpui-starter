import type { APIRoute } from 'astro';

export const GET: APIRoute = ({ site }) => {
  const sitemapURL = new URL('sitemap-index.xml', site ?? 'https://gpui-starter.freeoxide.com');

  return new Response(
    `User-agent: *
Allow: /

Sitemap: ${sitemapURL.href}
`
  );
};
