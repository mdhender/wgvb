//! Bounded viewport rendering for WGVB worlds.
//!
//! Generation is effectively unbounded; rendering is not. Every render request
//! defines a finite viewport and an explicit pixel scale. Renderer pixel
//! coordinates must never feed back into terrain generation.
//! See `DESIGN.md` section 29.

/// Palette and symbol-rule version. Cached or golden-compared rendered output
/// is invalid across a change to this value.
pub const RENDER_VERSION: u32 = 1;
