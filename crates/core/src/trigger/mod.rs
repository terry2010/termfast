//! Trigger engine module — Phase 4
pub mod template;
pub mod engine;
pub mod ipcheck;
pub mod health;

pub use template::render_template;
pub use engine::{TriggerEngine, TriggerEvent};
pub use ipcheck::IpChangeDetector;
pub use health::HealthChecker;
