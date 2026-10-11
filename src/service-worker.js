// Copyright (c) 2026 Witalis Domitrz <witekdomitrz@gmail.com>
// AGPL License
// Browser lifecycle/cache plumbing only; all app logic remains Rust.
//
// The whole worker is install, activate, fetch. Four rules hold it together:
//
// * Every path is relative to `self.location`, so the app installs and works
//   from whatever subdirectory it is published under.
// * The cache name is content-versioned by `build.rs`, which hashes the bytes
//   of every other shell file — the wasm included — and this template. Change
//   the app, or this caching logic, and the cache name changes with it.
// * No `skipWaiting`. An update takes over once the old tabs are gone, so a
//   live app never swaps the wasm out from under itself.
// * The fetch handler only ever answers for a URL inside this app's own
//   directory, checked on every request rather than assumed from the scope.
//   Scope is a registration's claim, not a promise, and these apps share one
//   origin with pages that are not this app.
// * The manifest is the one precached URL the fetch handler never answers
//   from the cache. It decides what an install is, so it must always be a
//   fresh read: a cached copy would let a fresh install re-derive the app's
//   identity from a manifest older than the last change to it.

const ROOT = new URL('./', self.location.href);
const CACHE = 'pwa-template-' + ROOT.pathname + '-__VERSION__';
const ASSETS = ['./', 'app.js', 'app_bg.wasm', 'manifest.webmanifest', 'icon-192.png', 'icon-512.png', 'icon.svg', 'index.html'].map(p => new URL(p, ROOT).href);
const IS_OWN = url => url.startsWith(ROOT.href);
// The manifest's URL on its own. It stays in ASSETS — a manifest that 404s
// should still fail the install, loudly — but it is the one precached URL the
// fetch handler will never answer out of the cache (see the fourth rule).
const MANIFEST = new URL('manifest.webmanifest', ROOT).href;

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
  // An allowlist, not a catch-all: this worker has no business touching anything
  // it did not precache, including the pages the user navigates away to. The
  // directory check is the second half of the same rule.
  //
  // The manifest is the one own-directory GET that always goes to the network.
  // It is what the browser reads to decide what app an install *is*: while a
  // cached copy answered, a fresh install of this app could read a manifest
  // from before any change to it, and derive the same stale identity the
  // family once shared — the very bug the identity fix was meant to retire.
  // The manifest is tiny; no offline entry point needs it cached.
  const url = event.request.url;
  if (url === MANIFEST) return;
  if (event.request.method !== 'GET' || !IS_OWN(url) || !ASSETS.includes(url)) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    const cached = await cache.match(event.request);
    return cached || fetch(event.request);
  })());
});