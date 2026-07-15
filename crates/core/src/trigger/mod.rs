//! Trigger engine module — Phase 4
pub mod engine;
pub mod health;
pub mod ipcheck;
pub mod template;

pub use engine::{TriggerEngine, TriggerEvent};
pub use health::HealthChecker;
pub use ipcheck::IpChangeDetector;
pub use template::render_template;
