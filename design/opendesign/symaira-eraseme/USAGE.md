# Symaira EraseMe design package usage

## Read order

1. `START_HERE.md`
2. `PRODUCT_CONTEXT.md` or `PRODUCT_DECISIONS.md`
3. `DESIGN.md`
4. `tokens.css` and `design-tokens.json`
5. `components.manifest.json`, then `components.html`
6. `preview/` and `asset-board.html`
7. `assets/manifest.json` and `assets/provenance.json` before export

## Design highlights

- Product: Symaira EraseMe
- Surface: Native macOS + CLI/MCP
- Primary accent: `#FF9A8F`
- Public copy: German and English variants are kept separate.

## Do

- Keep editable SVG sources beside PNG exports.
- Use synthetic demo data and exact current product boundaries.
- Re-capture store screenshots from the shipping app; never pass prototype screenshots off as release evidence.

## Avoid

- Do not overwrite canonical repository assets from OpenDesign without a reviewed integration change.
- Do not imply cloud, billing, hosted, or GUI capabilities the product does not own.
- Do not place secrets, personal data, real local paths, or production screenshots with private content in assets.
