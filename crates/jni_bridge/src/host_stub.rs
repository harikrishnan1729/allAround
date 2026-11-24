//! Non-Android stand-in so the JNI layer compiles, lints and unit-tests on a
//! workstation. Nothing here touches hardware.

use facetilt_core::StatsSnapshot;
use facetilt_decision::Rotation;
use jni::objects::GlobalRef;
use jni::JavaVM;

use crate::config::BridgeConfig;

pub fn start(_vm: JavaVM, _listener: GlobalRef, _package: &str, _rotation: Rotation, _cfg: BridgeConfig) -> bool {
    log::warn!("FaceTilt native engine is only available on Android");
    false
}

pub fn stop() {}

pub fn set_config(_cfg: BridgeConfig) {}

pub fn set_display_rotation(_r: Rotation) {}

pub fn report_applied(_us: u32) {}

pub fn stats() -> [i64; StatsSnapshot::LEN] {
    StatsSnapshot::default().to_array()
}

pub fn front_camera_id() -> Option<String> {
    None
}
