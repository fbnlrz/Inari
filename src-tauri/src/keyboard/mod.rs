//! SteelSeries **Apex** keyboard support: per-key RGB, the 128x40 OLED and the
//! settings the firmware exposes.
//!
//! The protocol was worked out against a real Apex Pro TKL Wireless (2023) —
//! [`protocol`] records exactly which parts are measured and which are taken on
//! trust from OpenRGB and `apex-tux`.

pub mod effects;
pub mod keys;
pub mod manager;
pub mod oled;
pub mod oled_modes;
pub mod protocol;
pub mod scenes;

pub use manager::KeyboardManager;
