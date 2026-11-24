# Symaira EraseMe release asset guide

| Target | File | Status |
|---|---|---|
| GitHub repository social preview | `assets/social/github-social-en.png` | Ready |
| Open Graph / Reddit / link cards | `assets/social/open-graph-de.png` | Ready |
| Square social post | `assets/social/square-de.png` | Ready |
| Portrait social post | `assets/social/portrait-de.png` | Ready |
| Story/reel cover | `assets/social/story-de.png` | Ready |
| Release announcement | `assets/release/release-card-en.png` | Ready |
| README hero | `assets/readme/readme-hero-de.png` | Ready |
| UI/UX reference | `eraseme-companion.html` | Existing reference |
| Store screenshots | `assets/store/SCREENSHOT_PLAN.md` | Capture from release build |

## Export rule

The `.svg` file is the editable source; the adjacent `.png` is the delivery export. Repository assets remain canonical until a deliberate repo integration copies a reviewed OpenDesign export back.

## Pre-publish gate

1. Verify product version, platform support, license, and availability against the repository.
2. Re-render localized copy if the release message changes.
3. Check image dimensions, alpha/background, thumbnail readability, and contrast.
4. Scan screenshots and terminal captures for secrets, identifiers, private paths, cookies, tokens, or personal data.
5. Link release notes to a real published tag; never write a version number into the evergreen master assets.
