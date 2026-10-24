//! Virtual synchronization module for metadata trees, hierarchy seeding, and on-demand file hydration.

pub mod engine;
pub mod entry;
pub mod seed;

pub use engine::VirtualSyncEngine;
pub use entry::{VirtualEntrySummary, make_dav_path};
pub use seed::{HierarchySeeder, StandardNextcloudSeeder};
