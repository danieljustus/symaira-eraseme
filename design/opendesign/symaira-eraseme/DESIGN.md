---
version: alpha
name: Symaira UI Foundation
description: Champagne-gold and warm-black Symaira identity with native Apple interaction, restrained Liquid Glass, and accessible semantic states.
colors:
  primary: "#E5C397"
  secondary: "#F8E6CD"
  tertiary: "#D4B285"
  neutral: "#0D0C0A"
  surface: "#171510"
typography:
  display:
    fontFamily: "SF Pro Display, -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: 34px
    fontWeight: 600
    lineHeight: 1.1
  title:
    fontFamily: "SF Pro Display, -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: 28px
    fontWeight: 600
    lineHeight: 1.2
  body:
    fontFamily: "SF Pro Text, -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: 17px
    fontWeight: 400
    lineHeight: 1.4
  caption:
    fontFamily: "SF Pro Text, -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: 12px
    fontWeight: 400
    lineHeight: 1.3
rounded:
  control: 10px
  card: 16px
  panel: 20px
spacing:
  xs: 4px
  sm: 8px
  md: 12px
  lg: 16px
  xl: 24px
  section: 32px
  spacious: 48px
---

## Overview

Symaira is a product family with native macOS/iOS clients and standalone CLI/MCP products. This project-local specification is the normative Symaira layer. The attached OpenDesign catalog system supplies only a platform baseline; it must not replace Symaira brand tokens or product boundaries.

The visual identity is champagne-gold and warm-black with warm neutral light mode, a restrained technical atmosphere, and a small amount of material depth. Brand expression comes from hierarchy, whitespace, the subtle blueprint/dot grid, and purposeful semantic accents—not decorative effects on every surface.

## Colors

Dark canvas uses `#0D0C0A`, deep canvas `#070605`, primary text `#F5F4F0`, secondary text `#B5AEA5`, and readable muted text `#837D74`. Light canvas uses `#F7F3EC`, secondary canvas `#EEE7DC`, opaque surface `#FFFCF7`, primary text `#211D17`, secondary text `#5F584F`, and muted text `#756D63`.

Brand colors are champagne primary `#E5C397`, champagne secondary `#F8E6CD`, gold shadow `#C29965`, ice/sand primary `#EEDCC4`, and ice/sand secondary `#D4B285`. Semantic colors are positive `#7FD49A`, warning `#E9BC73`, critical `#FF8A80`, and informative `#8CC8F2`.

Do not introduce Apple blue or unrelated accent palettes as Symaira brand colors. Platform exceptions remain inside native controls and do not redefine the product system.

## Typography

Native clients use platform semantic system text styles and Dynamic Type: display, title, heading, subheading, body, body emphasized, callout, secondary, caption, section label, mono, and mono small. The front-matter values are pull-layer reference values for prototypes; native clients retain platform semantic sizing. Commands, paths, hashes, and identifiers use a monospaced face. The Roboto Flex wordmark is a separate brand asset and is not general UI typography.

Section labels may use restrained tracking. Do not add tracking indiscriminately, freeze text to inaccessible fixed sizes, or mix decorative font families into a native product surface.

## Layout & Spacing

Use the shared 4/8/12/16/24/32/48px rhythm. Standard control radius is 10px, card radius 16px, and panel radius 20px. Keep important reading content inside a 760px readable column on wide Mac windows while lists and grids may expand outside it.

On iOS, interactive controls are at least 44pt high. On macOS, the shared minimum is 34pt. Prefer native split views, tabs, toolbars, sheets, lists, and controls over web-like imitations.

## Material & Components

Use the smallest amount of glass needed to make hierarchy clear. `SymairaBackdrop` owns the branded canvas and subtle grid. Content/data surfaces use standard Apple material through `glassCard` and `glassmorphicPanel`; these are not Liquid Glass controls. Liquid Glass is reserved for chrome and interactive controls through `symairaButtonStyle`, `symairaGlassChrome`, and `SymairaGlassEffectContainer` on supported systems.

Use `SymairaBadge`, `SymairaNotice`, `SymairaEmptyState`, and `SymairaLoadingState` for consistent semantic feedback. Every interactive element needs default, hover where supported, focus-visible, pressed, disabled, loading, error, and destructive-confirmation behavior where relevant.

## Platform Guidance

Native apps keep their own product identity and interaction model while consuming the shared `symaira-appkit` foundation. Separate products are not merged into a new hub. CLI/MCP products use the same semantic colors and typography where they expose a GUI, terminal presentation, documentation, or operator surface, but they do not gain an invented dashboard merely to look like an app.

Product-specific glyphs belong to the owning product. The family-level shell, safe zones, and icon geometry stay shared.

## Accessibility & Motion

Semantic colors adapt to light/dark appearance. Reduce Transparency replaces materials with opaque warm Symaira surfaces. Increase Contrast strengthens borders. Reduce Motion removes non-essential scale and transition effects. Decorative grids, glows, and telemetry corners are hidden from accessibility and never intercept input.

Preserve native keyboard semantics, visible focus treatment, Dynamic Type/font scaling, VoiceOver labels, and touch targets. Do not communicate state through color, glow, or translucency alone.

## Do's and Don'ts

### Do

- Keep champagne/sand accents purposeful and sparse.
- Let native platform controls supply familiar behavior.
- Separate content surfaces from interactive Liquid Glass chrome.
- Preserve light/dark, accessibility, offline, empty, loading, and error states.
- Follow the product-specific context file beside this document.

### Don't

- Do not use Apple blue as a substitute for Symaira's brand accent.
- Do not apply Liquid Glass to every content card.
- Do not invent a new cross-product hub or generic launcher.
- Do not duplicate Theme, runner, registry, or credential logic in an app.
- Do not put secrets, real vault contents, or private user data into prototypes.

## Source of Truth

Derived from `/Users/daniel/Dev/Symaira Dev/symaira-appkit/DESIGN.md`, `Sources/SymairaTheme/SymairaTheme.swift`, `Sources/SymairaTheme/Typography.swift`, `Sources/SymairaTheme/Components.swift`, and `Brand/README.md`. Product scope is defined in the adjacent `PRODUCT_CONTEXT.md` and the current Symaira workspace `AGENTS.md`.