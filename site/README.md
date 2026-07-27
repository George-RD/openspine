# OpenSpine site

Astro + Starlight site for OpenSpine. The custom marketing landing page lives in `src/pages/index.astro`; documentation routes remain in Starlight under `src/content/docs/`.

```sh
npm ci
npm run dev      # local server at http://localhost:4321/openspine/
npm run build    # production build to ./dist/
npm run preview
```

## Design context

- [`PRODUCT.md`](PRODUCT.md) records product truth, audience, proof, voice, and claims to avoid.
- [`DESIGN.md`](DESIGN.md) records the Impeccable-derived visual system and interaction rules.
- `src/styles/landing.css` is the landing-page entry point; its smaller modules separate shell, trace, scenario, sections, and responsive behavior.
- `src/styles/starlight.css` carries the same system into the documentation without the landing-page effects.

## Deployment

Pull requests that change `site/**` run the site build check. Changes merged to `main` deploy to GitHub Pages through `.github/workflows/deploy-site.yml`.
