# Project website

The website for this repository: a [Next.js](https://nextjs.org/) (App Router, TypeScript) site exported as static files
for GitHub Pages. The site code is MIT-licensed; the page text is CC BY-SA 4.0 like the Markdown it comes from (see
`NOTICE`).

**The Markdown in the repository is the only source.** Pages are rendered at build time; nothing is copied into `site/`:

| Source | Route |
|---|---|
| `README.md` | `/docs/` (overview) |
| `docs/<path>.md`, every file, picked up automatically | `/docs/<path>/` |
| `<dir>/README.md` and `<dir>/<NAME>.md` outside `docs/` | `/docs/repo/<dir>/`, `/docs/repo/<dir>/<name>/` |
| `CONTRIBUTING.md`, `SECURITY.md`, `TRADEMARKS.md` | `/docs/repo/<name>/` |
| README hardware table, `docs/test-plan.md`, `docs/test-results.md`, `kernel/PROVENANCE.md`, `kernel/patches/` | facts on the landing page `/` |

Which files exist is asked of git (tracked files and new files that are not ignored). [`lib/docs.ts`](lib/docs.ts) holds
the navigation groups and order and the hidden files. Relative links between rendered files become site routes
(anchors kept, GitHub-style heading ids); links to any other repository file go to its GitHub page. Images that
Markdown references by relative path are copied to `public/_repo/` by `scripts/copy-doc-assets.mjs` before each build.
`scripts/third-party-licenses.mjs` writes `public/third-party-licenses.txt` (the license files of the site's
production dependencies, linked from the footer) before each build.

The landing page readers in [`lib/facts.ts`](lib/facts.ts) stop the build with a message naming the file and the row
when a table they read changes shape (a new verdict word, a test result for an ID the plan does not define, a
missing column). Adapt the reader or the document; a page with wrong numbers is not published.

## Develop

Node.js 20.9 or newer (the workflow uses the current LTS).

```sh
cd site
corepack enable    # once: provides the pnpm version pinned in package.json
pnpm install
pnpm run dev        # http://localhost:3000; edits to the Markdown show on reload
pnpm run typecheck
```

## Build and check

```sh
PAGES_BASE_PATH=/surface-pro-9-linux pnpm run build    # static site in site/out/
PAGES_BASE_PATH=/surface-pro-9-linux pnpm run check    # every document has a page; links, anchors, images resolve
```

`check` also verifies that links to repository files on GitHub name paths that exist. Other build-time variables:
`REPO_URL` (default `https://github.com/joonhoekim/surface-pro-9-linux`) and `REPO_BRANCH` (default `main`) for links
to files, `REPO_ROOT` (default: the parent of `site/`).

## Deploy

[`.github/workflows/pages.yml`](../.github/workflows/pages.yml) builds and checks `site/` on every push to `main` and on
pull requests that touch the documentation or the site, and publishes `site/out` with the official Pages actions
(`configure-pages` supplies the base path) from `main` or by hand (Actions, "Website", Run workflow). It needs
Settings, Pages, Source: **GitHub Actions**; until Pages is enabled the workflow only builds and checks the site.
