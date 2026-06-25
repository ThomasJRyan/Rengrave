# Product

## Register

product

## Users

R-Engrave is for CNC users who need to generate reliable engraving and v-carving toolpaths from text, font, bitmap, DXF, or SVG inputs. Users are working in a desktop machining workflow where dimensions, file compatibility, and output confidence matter more than presentation.

## Product Purpose

R-Engrave is a Rust port of F-Engrave that preserves legacy settings behavior, file compatibility, and generated toolpaths while providing a maintainable native desktop UI. Success means users can reproduce trusted F-Engrave-style output through the CLI or UI, inspect it clearly, and export CNC-ready files without hidden drift between interactive and batch workflows.

## Brand Personality

Reliable, fast, functional. The interface should feel like purposeful shop software: compact, direct, predictable, and focused on getting valid machining output.

## Anti-references

Do not make R-Engrave feel like a webpage or marketing surface. Avoid hero layouts, decorative imagery, oversized typography, promotional copy, empty visual flourish, and controls arranged for visual novelty instead of task flow. Every visible element should earn its place in the tool workflow.

## Design Principles

- Preserve compatibility before polish: F-Engrave parity and generated-output confidence are the primary product contract.
- Put controls where the workflow needs them: settings, preview, output, status, and logs should be arranged for repeated desktop use.
- Make machine state obvious: stale output, warnings, calculation progress, disabled controls, and export readiness must be legible at a glance.
- Prefer dense clarity over decoration: use compact panels, precise labels, stable control sizing, and familiar desktop affordances.
- Keep CLI and UI behavior aligned: interactive choices should map clearly to the same core settings and output path used by batch mode.

## Accessibility & Inclusion

Target the usual desktop accessibility baseline: WCAG AA contrast for text and state indicators, color-blind-safe toolpath layers, reduced-motion-safe interactions, high-contrast-friendly themes, and keyboard-accessible workflows where egui supports them. Never rely on color alone for critical machining or output state.
