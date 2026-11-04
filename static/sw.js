const CACHE = 'llibrary-v3';
const ASSETS = [
  '/static/style.css',
  '/static/manifest.webmanifest',
  '/static/icon-192.png',
  '/static/placeholder.svg',
  '/static/htmx.min.js',
  '/static/reader.css',
  '/static/reader.js',
  '/static/epubjs/epub.min.js',
  '/static/epubjs/jszip.min.js',
  '/static/pdfjs/pdf.min.js',
  '/static/pdfjs/pdf.worker.min.js',
  '/static/reader-pdf.js',
  '/static/reader-txt.js'
];

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(CACHE).then((cache) => cache.addAll(ASSETS)).catch(() => undefined));
  self.skipWaiting();
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(keys.filter((key) => key !== CACHE).map((key) => caches.delete(key)))
    )
  );
});

self.addEventListener('fetch', (event) => {
  const url = new URL(event.request.url);
  if (event.request.method !== 'GET') return;
  if (url.pathname.startsWith('/files/') || url.pathname.startsWith('/covers/')) return;
  if (url.pathname.startsWith('/static/')) {
    event.respondWith(caches.match(event.request).then((cached) => cached || fetch(event.request)));
  }
});
