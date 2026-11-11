# Golden-ratio design system (design-1110g)

Amara OS lays out on φ = 1.618 and the Fibonacci numbers near it, across the whole app (Home already did).

## Spacing: Fibonacci 3 · 5 · 8 · 13 · 21 · 34 · 55 · 89 px
Defined once in `src-console/styles/tokens.css` as `--fib-3 … --fib-89`. The existing `--space-*` names point at the nearest step, so components didn't need renaming:

| Name | Was | Now |
|---|---|---|
| `--space-0_5` | 2 | 3 |
| `--space-1` | 4 | 5 |
| `--space-2` | 8 | 8 |
| `--space-3`, `--space-4` | 12, 16 | 13 |
| `--space-5`, `--space-6` | 20, 24 | 21 |
| `--space-8`, `--space-10` | 32, 40 | 34 |
| `--space-12`, `--space-16` | 48, 64 | 55 |
| `--space-24` (new) | n/a | 89 |

`--space-10` and `--space-16` moved here from glass.css. `--pad-card` is 13 and `--inset-content` is 21. Literal px in `margin`/`padding`/`gap` (and spacing custom properties) were snapped to the nearest step; ties go up. 0 and 1px hairlines are allowed.

## Splits
- Two-column, hero and detail layouts use the golden section, 1.618fr : 1fr with the main column the larger part: Neuro rows (3:2 → φ), Neuro history split (320px | 1fr → 1 : φ) and sensors (140px | 1fr → 1 : φ), Lists form rows (1:2 → 1 : φ), Today half row (1:1 → φ : 1).
- Equal grids of the same kind of thing stay equal: the Fitness 2×2 charts, tiles and strips.
- The design-1110d/e/h pages (Business, Crews, Neuro history recordings, Sleep card) only took the spacing tokens, as briefed.
- Page column: `--page-column: clamp(610px, 61.8vw, 987px)`, which is 1/φ of the window with Fibonacci bounds. Fitness and Neuro use it too.

## Type: 11 · 13 · 17 · 21 · 27 · 34 (13 × √φ per step)
- Roles: subheadline and badge 11, body, headline and callout 13, title2 and title3 17, title1 21, large title 27, display 34.
- Nothing is under 11px; badges went from 10 to 11.
- Literal font sizes were snapped to the scale.
- The segmented-control segment inset went from 2px to 3px, so its focus ring offset is 3px (glass-fallbacks e2e updated).

## Radius: 3 · 5 · 8 · 13 · 21
xs 3, sm 5, md 8 (cards), lg 13 (panels), xl 21 (popovers), plus pill.

## Enforced
`test/design/scale.test.ts` (runs in `npm test`) fails on:
- any spacing px that isn't on the scale in any `src-console` stylesheet
- any text under 11px

Named exceptions:
- Home's `--landing-pad-top: 56px` (Home keeps its golden layout).
- Window chrome, which lines up with the macOS traffic lights rather than the scale: `--shell-traffic-inset: 78px`, the wordmark's `calc(inset + 6px)` (ink at x = 84) and `styles/wordmark.css`. `test/e2e/wordmark.spec.ts` guards it unchanged.

## Not changed
- Home's golden layout (`LandingScreen.css`) and the HR graph.
- Window chrome geometry (traffic-light inset, wordmark lockup).
- Inline TSX geometry (chart and SVG sizes).
- Positioning offsets (`top`/`left`) and borders, which aren't spacing.
