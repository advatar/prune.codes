# A2UI Versioning

## Current Support
- A2UI v0.9 is the default protocol target.
- A2UI v0.8 messages remain supported via the adapter.
- The runtime normalizes inbound messages into a single internal model.

## v0.9 Support
- v0.9 support is implemented via adapters and is selected when v0.9 message keys are present.
- The renderer reads only the normalized model, so switching versions does not require UI changes.

## Key Diffs We Normalize
- Message renames: `beginRendering` -> `createSurface`, `surfaceUpdate` -> `updateComponents`.
- Component shape: v0.8 wrapper `{ "component": { "Text": { ... } } }` vs v0.9 flat `"component": "Text"`.
- Data model updates: v0.8 typed values vs v0.9 native JSON + JSON Pointer paths.

## Version Selection
1) If v0.9-only keys are present, decode as v0.9.
2) If v0.8-only keys are present, decode as v0.8.
3) If only shared keys are present (for example `deleteSurface`), default to v0.9 unless configured otherwise.
