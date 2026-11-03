// Minimal service worker: makes Neko AI installable as an app. It deliberately caches nothing,
// so you never see a stale UI or stale chats; everything comes fresh from your server.
self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));
self.addEventListener("fetch", () => {});
