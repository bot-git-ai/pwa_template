// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License
// Browser lifecycle/cache plumbing only; all app logic remains Rust.
//
// The whole worker is install, activate, fetch. Three rules hold it together:
//
// * Every path is relative to `self.location`, so the app installs and works
//   from whatever subdirectory it is published under.
// * The cache name is content-versioned by `build.rs`, which hashes the bytes
//   of every other shell file — the wasm included — and this template. Change
//   the app, or this caching logic, and the cache name changes with it.
// * No `skipWaiting`. An update takes over once the old tabs are gone, so a
//   live app never swaps the wasm out from under itself. A user mid-session
//   gets the app they started with, then the new one on the next launch.

const ROOT = new URL('./', self.location.href);
const CACHE = 'pwa-template-' + ROOT.pathname + '-__VERSION__';
const ASSETS = ['./', 'app.js', 'app_bg.wasm', 'manifest.webmanifest', 'icon-192.png', 'icon-512.png', 'icon.svg', 'index.html'].map(p => new URL(p, ROOT).href);

self.addEventListener('install', event => {
  event.waitUntil(caches.open(CACHE).then(cache => cache.addAll(ASSETS)));
});

self.addEventListener('activate', event => {
  event.waitUntil((async () => {
    const prefix = 'pwa-template-' + ROOT.pathname + '-';
    for (const key of await caches.keys()) {
      if (key.startsWith(prefix) && key !== CACHE) await caches.delete(key);
    }
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', event => {
  // Deliberately leave everything else alone. An allowlist, not a
  // catch-all: this worker has no business touching anything it did not
  // precache, including the pages the user navigates away to.
  if (event.request.method !== 'GET' || !ASSETS.includes(event.request.url)) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const cached = await cache.match(event.request);
    return cached || fetch(event.request);
  })());
});