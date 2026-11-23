import rows from './catalog.json';
import type { Tweak } from './types';

// This JSON catalog is shared with the Rust command layer and is the one
// allowlist used by both the UI and the native backend.
export const catalog = rows as Tweak[];
