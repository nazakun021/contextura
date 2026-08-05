# Glossary

## Overlay placement

The position of an English translation relative to its detected Japanese text
rectangle.

- **Cover:** The translation is drawn at the exact original text rectangle
  coordinates. This is the default behavior; overlapping Styled Boxes are not
  collision-shifted in this mode.
- **Above:** The translation is drawn above the original text rectangle, with a
  six-pixel gap and a top-edge clamp.
- **Below:** The translation is drawn below the original text rectangle, with a
  six-pixel gap and a bottom-edge clamp.

## Model selection

Choosing an installed local GGUF model as the active translation model. The
runtime restarts after a model changes so the bundled `llama-server` loads the
selected model.
