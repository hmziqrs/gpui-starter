import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import tailwindcss from "@tailwindcss/vite";
import icon from "astro-icon";
import { readFileSync } from "fs";
import { join } from "path";
import sitemap from "@astrojs/sitemap";

function serveLocalAudio() {
  return {
    name: "serve-local-audio",
    configureServer(server) {
      server.middlewares.use("/audio", (req, res, next) => {
        const filePath = join(process.cwd(), "audio", req.url.replace(/^\//, ""));
        try {
          const data = readFileSync(filePath);
          res.setHeader("Content-Type", "audio/mpeg");
          res.end(data);
        } catch {
          next();
        }
      });
    },
  };
}

export default defineConfig({
  site: "https://gpui-starter.hmziq.xyz",
  srcDir: "./web",
  vite: {
    plugins: [tailwindcss(), serveLocalAudio()],
    build: {
      rollupOptions: {
        output: {
          manualChunks(id) {
            if (id.includes("node_modules/three")) return "three";
          },
        },
      },
    },
  },
  integrations: [
    sitemap({
      filter: (page) => !page.includes('/api/'),
    }),
    icon({
      include: {
        "simple-icons": ["github", "rust", "x", "linkedin", "telegram", "reddit"],
        lucide: ["globe"],
      },
    }),
    starlight({
      title: "gpui-starter",
      description: "A production-ready Rust boilerplate for GPUI desktop apps with themes, i18n, forms, and more",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/freeoxide/gpui-starter",
        },
      ],
      editLink: {
        baseUrl: "https://github.com/freeoxide/gpui-starter/edit/master/",
      },
      sidebar: [
        {
          label: "Getting Started",
          items: [{ slug: "docs/getting-started" }],
        },
        {
          label: "Features",
          items: [
            { slug: "docs/themes" },
            { slug: "docs/i18n" },
            { slug: "docs/forms" },
            { slug: "docs/command-launcher" },
            { slug: "docs/notifications" },
            { slug: "docs/secure-storage" },
            { slug: "docs/gpui-query" },
            { slug: "docs/auto-updater" },
            { slug: "docs/crash-reporting" },
            { slug: "docs/diagnostics" },
            { slug: "docs/telemetry" },
            { slug: "docs/undo-redo" },
            { slug: "docs/single-instance" },
            { slug: "docs/websocket" },
            { slug: "docs/error-boundaries" },
          ],
        },
        {
          label: "Guides",
          items: [
            { slug: "forms" },
          ],
        },
        {
          label: "Architecture",
          items: [
            { slug: "docs/architecture" },
            { slug: "docs/routing" },
            { slug: "docs/testing" },
            { slug: "docs/performance" },
          ],
        },
      ],
      customCss: ["/web/styles/starlight.css"],
      lastUpdated: true,
      head: [
        { tag: 'meta', attrs: { property: 'og:title', content: 'gpui-starter Documentation' } },
        { tag: 'meta', attrs: { property: 'og:description', content: 'A production-ready boilerplate for building desktop apps with GPUI' } },
        { tag: 'meta', attrs: { property: 'og:type', content: 'website' } },
        { tag: 'meta', attrs: { property: 'og:site_name', content: 'gpui-starter' } },
        { tag: 'meta', attrs: { property: 'og:locale', content: 'en_US' } },
        { tag: 'meta', attrs: { property: 'og:image', content: 'https://gpui-starter.hmziq.xyz/og-image.png' } },
        { tag: 'meta', attrs: { property: 'og:image:width', content: '1200' } },
        { tag: 'meta', attrs: { property: 'og:image:height', content: '630' } },
        { tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } },
        { tag: 'meta', attrs: { name: 'twitter:site', content: '@hmziqrs' } },
        { tag: 'meta', attrs: { name: 'twitter:creator', content: '@hmziqrs' } },
        {
          tag: 'script',
          content: `requestIdleCallback(()=>{import('https://www.gstatic.com/firebasejs/12.13.0/firebase-app.js').then(({initializeApp})=>{const c={apiKey:'AIzaSyDI-CutFk3prIj64gQfz332Cnrvh3xeUfc',authDomain:'gpui-starter.firebaseapp.com',projectId:'gpui-starter',storageBucket:'gpui-starter.firebasestorage.app',messagingSenderId:'117315648896',appId:'1:117315648896:web:4291ed5b219b49d7cfd565',measurementId:'G-9KJX86QRFG'};const a=initializeApp(c);import('https://www.gstatic.com/firebasejs/12.13.0/firebase-analytics.js').then(({getAnalytics})=>{getAnalytics(a)})})});`,
        },
      ],
      favicon: "/favicon.svg",
    }),
  ],
});
