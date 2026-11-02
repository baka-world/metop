//! metop: Linux memory analysis TUI.
//!
//! Two-tier sampling, driven by measured costs:
//!   tier 1 (every tick): /proc/<pid>/statm, ~15-30 ms for the whole system
//!   tier 2 (budgeted):   /proc/<pid>/smaps_rollup, 1.5 ms avg, up to 19 ms
//!   detail   (on demand): /proc/<pid>/smaps, 30-40 ms

pub mod app;
pub mod apps;
pub mod cgroup;
pub mod diff;
pub mod i18n;
pub mod json;
pub mod model;
pub mod procfs;
pub mod sampler;
pub mod theme;
pub mod ui;
