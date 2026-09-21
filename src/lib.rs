//! Core library for the native `one_big_script_rs_all_in_one` application.
//!
//! Suggested reading order for newcomers:
//! 1. `main.rs` or `bin/batch_run.rs` — entrypoints.
//! 2. `cli.rs` — startup parameters.
//! 3. `ui/app.rs` — GUI shell and background task orchestration.
//! 4. `modes/*` — business logic for each report type.
//! 5. `paths/` — path resolution for DBF/XLSX inputs and output folders.
//! 6. `domain/` — field codes and labels shared by the UI and the modules.
//!
//! Layering rule: `modes/*` may use `domain`, `paths` and `tabular`, but never
//! `ui` or `cli`.

pub mod cli;
pub mod domain;
pub mod modes;
pub mod paths;
pub(crate) mod tabular;
pub mod ui;
