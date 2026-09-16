# ADR-0050: Panel typeface family is a family *class*

- **Status**: Accepted
- **Date**: 2026-09-11
- **Deciders**: Core Maintainers

## Context

`display.font_family` was documented as an arbitrary primary font family name:

> Otherwise the named family is the "first font": it wins for every codepoint it
> covers, and missing codepoints fall back to the system fonts that do cover
> them. (`docs/reference/configuration.md`)

The settings application exposed it as a free-text field, `platform.toml.example`
suggested `font_family = "Noto Sans CJK SC"`, and ADR-0016 relied on it as the
per-glyph fallback entry point.

The implementation did nothing. `TextRaster::set_preferred_family` was an empty
function whose doc comment described it as "a no-op kept only for source
compatibility with the former ab_glyph rasteriser", and the panel called it
whenever the family string changed. A user could type any family name, watch the
setting persist, and see no rendering difference at all.

The root cause is a capability mismatch: flux-text resolves faces through
fontconfig and exposes `flux_text_set_default_family`, which takes a
`flux_text_family` **enum** (`DEFAULT` / `SANS` / `SERIF` / `MONO`) and has no
free-form family selector. Arbitrary family pinning is not something the text
stack can do.

## Decision

Model the setting as the capability that actually exists: a family **class**.

- Introduce `FontFamilyClass` in `typio-host-types` with the four values the
  text stack supports (`default`, `sans`, `serif`, `mono`), a `parse` that
  accepts the canonical spellings and their common aliases (`sans-serif`,
  `monospace`, `""`), and an `as_str` for the canonical spelling.
- `PanelFontConfig.family` becomes a `FontFamilyClass`; `family_opt()` is gone
  because there is no longer an "unset means arbitrary" state to represent.
- `TextRaster` stores the applied class and implements
  `set_preferred_family` for real by calling `flux_text_set_default_family`,
  with a `preferred_family()` accessor.
- The daemon config loader parses `display.font_family`, logs a warning and
  falls back to `default` on an unrecognised value.
- The settings application renders the setting as a dropdown
  (`System default` / `Sans` / `Serif` / `Monospace`) instead of a text field.
- `docs/reference/configuration.md`, `data/platform.toml.example`, and
  `docs/dev/panel-appearance.md` document the class and its per-codepoint
  fallback behaviour.

## Alternatives considered

- **Add a free-form family selector to flux-text.** Rejected here: it is an
  upstream API and ABI change in a sibling repository, and it is not required
  to satisfy the user-facing intent (choose the general typeface style of the
  candidate panel).
- **Delete the setting entirely.** Rejected: selecting a serif or monospace
  panel is a legitimate preference, and the class selector is implementable
  today with an existing API.
- **Keep the free-text field and apply only recognised values.** Rejected:
  leaves a control that accepts input which cannot take effect. A dropdown whose
  options are exactly the supported values cannot mislead.

## Consequences

- Positive: every value the UI offers and every value the config accepts
  changes rendering; the documented behaviour matches the implemented one.
- Positive: `FontFamilyClass::parse` is the single place that decides what
  counts as a valid `display.font_family`, and an invalid value is reported
  through the existing `typio.config` tracing target.
- Trade-off: a user who wrote an exact family name (for example
  `font_family = "Noto Sans CJK SC"`) loses that spelling. It never had an
  effect, and the value now reads back as `default`. CJK glyphs continue to
  render through fontconfig's per-codepoint fallback regardless of class.
- Negative (accepted): the setting is less expressive than its former
  documentation implied. Recovering arbitrary family pinning would require an
  upstream flux-text capability; if that lands, this ADR should be superseded
  rather than silently extended.
