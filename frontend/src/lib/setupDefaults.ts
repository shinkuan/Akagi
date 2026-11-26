// First-run wizard seeding.
//
// The Rust default is already the embedded backend (`src/config/capture.rs`),
// so a fresh config arrives correct. This module covers the one case the
// default cannot: a config written by an older build (whose default was
// `mitm`) that has not completed first-run setup yet — those users get the
// same one-window experience as a new install.

import { platformInfo } from './platforms'
import type { AppConfig } from '@/types'

/**
 * Pre-select the embedded backend for users whose config still carries the old
 * `mitm` default.
 *
 * Only when every condition holds:
 *   - setup has never been completed (`first_run_completed === false`) — a
 *     Settings re-run must keep the user's saved mode, not reset it;
 *   - the platform has a web client to embed — native-only platforms such as
 *     Riichi City are MITM-only;
 *   - the mode is still the old untouched default (`mitm`) — we never
 *     override a mode a previous step/seed already chose.
 *
 * Returns the config unchanged when any condition fails, so it is safe to run
 * on every draft seed (idempotent).
 */
export function withFirstRunCaptureDefault(config: AppConfig): AppConfig {
  if (config.general.first_run_completed) return config
  if (config.capture.mode !== 'mitm') return config
  if (!platformInfo(config.platform.kind).supportsChromium) return config
  return {
    ...config,
    capture: { ...config.capture, mode: 'embedded' },
  }
}
