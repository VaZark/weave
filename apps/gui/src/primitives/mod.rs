//! Headless logic behind the components: no drawing, no iced widgets, easy to test.
//!
//! Each file says whether it is **generic** (nothing about music; a candidate for upstreaming
//! into iced-adaptive-design, where `primitives/` holds the same kind of thing) or
//! **app-specific**. Keep generic ones free of imports from the rest of this crate.

pub mod bulk_tags;
pub mod synced;
