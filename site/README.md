# OpenSpine site

Astro + Starlight docs site for OpenSpine. Isolated from the Rust
workspace's own `package.json`/gate — this directory has its own
dependencies and its own build.

```sh
npm install
npm run dev     # local dev server at localhost:4321
npm run build   # production build to ./dist/
```

Deployment is out of scope for this repository right now — `npm run
build` passing locally is the bar. Hosting (e.g. Cloudflare Pages/Workers)
is a follow-up to wire when ready.
