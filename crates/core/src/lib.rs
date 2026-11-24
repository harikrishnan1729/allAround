//! # facetilt-core
//!
//! The latency-critical control logic of FaceTilt, written as a pure,
//! deterministic state machine:
//!
//! ```text
//!  Event (sensor sample, camera opened, frame classified, tick, stop, …)
//!        │
//!        ▼
//!  Engine::handle(now_ms, event) ──► Commands (OpenCamera, CaptureFrame, CloseCamera, Apply)
//! ```
//!
//! The engine never touches hardware, clocks or threads; `facetilt-jni` feeds it
//! real events on Android and the tests in `tests/` feed it recorded/synthetic
//! sensor traces with a simulated camera. No `unsafe` code.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod config;
mod engine;
mod filter;
mod metrics;
mod ratelimit;
pub mod trace;

pub use config::EngineConfig;
pub use engine::{AbortReason, Command, Commands, ControlAction, Engine, Event, Phase};
pub use facetilt_decision as decision;
pub use filter::{angle_between_deg, LowPass};
pub use metrics::{ActivationRecord, Metrics, Outcome, StatsSnapshot};
pub use ratelimit::RateLimiter;
