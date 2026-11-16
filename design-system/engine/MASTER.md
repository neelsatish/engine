# Design System Master File

> **LOGIC:** When building a specific page, first check `design-system/pages/[page-name].md`.
> If that file exists, its rules **override** this Master file.
> If not, strictly follow the rules below.

---

**Project:** Engine
**Generated:** 2026-11-16 11:07:31
**Category:** Productivity Tool
**Design Dials:** Variance 2/10 (Centered / Minimal) | Motion 3/10 (Subtle) | Density 5/10 (Standard)

---

## Terminal tiles (v0.12, the user's direction; overrides older rules below where they differ)

The user asked for Engine to look like an Omarchy desktop (btop, fastfetch, a CRT terminal) in the Velvet Fold
colours. Rules:

- **Type:** JetBrains Mono Variable everywhere (bundled with Fontsource), 13px body, 12px labels, 11px meta.
  Weights 400 and 500 only; bold ruins a monospace face.
- **Tiles:** `fieldset.pane` with a `legend` that cuts the 1px `--pane-border` (#3a3566) like btop; legends are
  lowercase with a superscript number (¹ system, ² engine, ³ home, ⁴ chat). Background `--pane-bg`, corners 4px
  (`--r-sm` 2px, `--r-md` 3px, `--r-lg` 4px). A focused tile's border warms toward the accent.
- **Wordmark:** "ENGINE" in 6×7 pixel glyphs drawn as SVG cells, one Velvet gradient across the whole word, a
  #2a2650 drop shadow, `crispEdges`. It ripples column by column only while listening.
- **Meters:** 22 cells coloured by position (`--meter-lo` #6f7fc4, `--meter-mid` #8f92ee, `--meter-hi` #f0718a);
  history graphs from ▁▂▃▄▅▆▇█ with a soft glow.
- **CRT (chat only):** bezelled screen with an 18/22px radius, scanlines and a vignette as a non-interactive
  overlay, a faint text glow. Prompt `engine ❯`; your lines start with ❯, results with ✓ or ✗.
- Contrast: body text #c7c3dc to #f4f2ff on near-black stays above 7:1; muted #9f9ab8 is for meta only.

## Global Rules

### Color Palette

Engine is dark-mode only, in the user's **Velvet Fold** palette (from their own Claude theme): matte black-violet
surfaces, a periwinkle accent, and a lavender-to-blue-violet gradient for the logo, primary button and background
rain. This palette is the user's choice and outranks any generated one.

| Role | Hex | CSS Variable |
|------|-----|--------------|
| Background | `#0E0C15` | `--bg` |
| Surface (input, code) | `#13101C` | `--surface` |
| Surface raised (hover) | `#1D192A` | `--surface-2` |
| Glass panel over the rain | `rgb(19 16 28 / 0.74)` + 14px blur | `--glass` |
| Border | `#2A2540` | `--border` |
| Foreground | `#ECEAF6` | `--text` |
| Secondary foreground | `#C7C3DC` | `--text-secondary` |
| Muted foreground | `#9F9AB8` | `--text-muted` |
| Accent / focus ring | `#8E90F5` | `--accent` |
| Accent gradient | `#B9BBFF` to `#8F92EE` to `#6F7FC4` | `--accent-gradient` |
| On accent | `#0E0C15` | `--on-accent` |
| Success | `#5FD4A0` | `--success` |
| Destructive | `#F0718A` | `--danger` |

**Background rain:** an ASCII "matrix" of falling characters in the accent gradient, drawn on a canvas behind
everything at low strength. Content sits on glass panels so text never sits directly on the rain. It runs at about
20 fps only while Engine is visible and focused, and draws one still frame under reduced motion.

### One system (taste-skill)

Polished against taste-skill's rules, adapted for a desktop app. The palette above stays: it's the user's own, and a
project's design system outranks any skill (taste-skill's "never violet" rule is overruled here).

- **Radius:** 8px for controls (buttons, inputs, chips, tabs), 12px for menus and inner panels, 16px for cards and
  the dump box (`--r-sm`, `--r-md`, `--r-lg`). Nothing else.
- **Buttons:** one solid accent button for the main action in a view (`.button.primary`), one outlined style for
  the rest (`.button`), and icon buttons (`.icon-btn`) with an aria-label. No gradient, glow or shadowed buttons.
- **Motion:** every pressable thing steps up 1px on hover, and sinks 1px and scales to .985 on press: 150ms in,
  120ms out, 90ms press, on one curve, `cubic-bezier(.2,.8,.2,1)`. Hover moves only for a fine pointer. Cards without
  links don't move.
- **Type:** sentence case everywhere and no tracked capitals. Labels are 13px at 600; nothing reads below 12px, and
  12px only for meta (paths, badges, calendar text).
- **Colour:** the interface stays neutral, and the accent marks the main button, focus, live state and the logo.
  No glows on dots or buttons.
- **Copy:** buttons name a verb and an object ("Add project", "Install voice pack").

### Typography

- **Font:** Inter (variable), bundled locally with `@fontsource-variable/inter` because Engine must work offline.
  Fallback: `"Segoe UI Variable", system-ui, sans-serif`.
- Body 15px / 1.5, labels 12px uppercase with 0.08em tracking, input 18px.

### Spacing Variables

*Density: 5/10 — Standard*

| Token | Value | Usage |
|-------|-------|-------|
| `--space-xs` | `4px` / `0.25rem` | Tight gaps |
| `--space-sm` | `8px` / `0.5rem` | Icon gaps, inline spacing |
| `--space-md` | `16px` / `1rem` | Standard padding |
| `--space-lg` | `24px` / `1.5rem` | Section padding |
| `--space-xl` | `32px` / `2rem` | Large gaps |
| `--space-2xl` | `48px` / `3rem` | Section margins |
| `--space-3xl` | `64px` / `4rem` | Hero padding |

### Shadow Depths

| Level | Value | Usage |
|-------|-------|-------|
| `--shadow-sm` | `0 1px 2px rgba(0,0,0,0.05)` | Subtle lift |
| `--shadow-md` | `0 4px 6px rgba(0,0,0,0.1)` | Cards, buttons |
| `--shadow-lg` | `0 10px 15px rgba(0,0,0,0.1)` | Modals, dropdowns |
| `--shadow-xl` | `0 20px 25px rgba(0,0,0,0.15)` | Hero images, featured cards |

---

## Component Specs

### Buttons

```css
/* Primary Button */
.btn-primary {
  background: #EA580C;
  color: #000000;
  padding: 12px 24px;
  border-radius: 8px;
  font-weight: 600;
  transition: all 200ms ease;
  cursor: pointer;
}

.btn-primary:hover {
  opacity: 0.9;
  transform: translateY(-1px);
}

/* Secondary Button */
.btn-secondary {
  background: transparent;
  color: #134E4A;
  border: 2px solid #0D9488;
  padding: 12px 24px;
  border-radius: 8px;
  font-weight: 600;
  transition: all 200ms ease;
  cursor: pointer;
}
```

### Cards

```css
.card {
  background: #F0FDFA;
  border-radius: 12px;
  padding: 24px;
  box-shadow: var(--shadow-md);
  transition: all 200ms ease;
  cursor: pointer;
}

.card:hover {
  box-shadow: var(--shadow-lg);
  transform: translateY(-2px);
}
```

### Inputs

```css
.input {
  padding: 12px 16px;
  border: 1px solid #E2E8F0;
  border-radius: 8px;
  font-size: 16px;
  transition: border-color 200ms ease;
}

.input:focus {
  border-color: #0D9488;
  outline: none;
  box-shadow: 0 0 0 3px #0D948820;
}
```

### Modals

```css
.modal-overlay {
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(4px);
}

.modal {
  background: white;
  border-radius: 16px;
  padding: 32px;
  box-shadow: var(--shadow-xl);
  max-width: 500px;
  width: 90%;
}
```

---

## Style Guidelines

**Style:** Minimalism & Swiss Style

**Keywords:** Clean, simple, spacious, functional, white space, high contrast, geometric, sans-serif, grid-based, essential

**Best For:** Enterprise apps, dashboards, documentation sites, SaaS platforms, professional tools

**Key Effects:** Subtle hover (200-250ms), smooth transitions, sharp shadows if any, clear type hierarchy, fast loading

### Page Pattern

**Pattern Name:** Product Demo + Features

- **Conversion Strategy:** Use an interactive demo only when it explains value better than static media. Provide captions, transcript, visible play/pause controls, and a non-video fallback; do not autoplay under reduced motion. Pause media when offscreen or hidden and keep the final product state available as static content.
- **CTA Placement:** Video center + CTA right/bottom
- **Section Order:** Hero > Product video/mockup (center) > Feature breakdown per section > Comparison (optional) > CTA

---

## Motion

**Scroll Reveal** (Subtle) — Trigger: scroll (viewport enter) | Duration: 300-400ms | Easing: `power1.out`

```js
gsap.from(el, { opacity: 0, y: 12, duration: 0.35, ease: 'power1.out', scrollTrigger: { trigger: el, start: 'top 90%', toggleActions: 'play none none reverse' } });
```

**Framework notes:** Requires the ScrollTrigger plugin registered once via gsap.registerPlugin(ScrollTrigger); Use matchMedia('(prefers-reduced-motion: reduce)') to skip non-essential motion and render the final state immediately

- ✅ Keep the y offset small (8-16px) so it reads as a fade, not a slide
- ❌ Don't reveal below-the-fold content needed for SEO/crawlers as invisible-by-default without a no-JS fallback
- ⚡ toggleActions 'play none none reverse' avoids re-triggering on every scroll direction change

---

## Anti-Patterns (Do NOT Use)

- ❌ Complex onboarding
- ❌ Slow performance

### Additional Forbidden Patterns

- ❌ **Emojis as icons** — Use SVG icons (Heroicons, Lucide, Simple Icons)
- ❌ **Missing cursor:pointer** — All clickable elements must have cursor:pointer
- ❌ **Layout-shifting hovers** — Avoid scale transforms that shift layout
- ❌ **Low contrast text** — Maintain 4.5:1 minimum contrast ratio
- ❌ **Instant state changes** — Always use transitions (150-300ms)
- ❌ **Invisible focus states** — Focus states must be visible for a11y

---

## Pre-Delivery Checklist

Before delivering any UI code, verify:

- [ ] No emojis used as icons (use SVG instead)
- [ ] All icons from consistent icon set (Heroicons/Lucide)
- [ ] `cursor-pointer` on all clickable elements
- [ ] Hover states with smooth transitions (150-300ms)
- [ ] Light mode: text contrast 4.5:1 minimum
- [ ] Focus states visible for keyboard navigation
- [ ] `prefers-reduced-motion` respected
- [ ] Responsive: 375px, 768px, 1024px, 1440px
- [ ] No content hidden behind fixed navbars
- [ ] No horizontal scroll on mobile
