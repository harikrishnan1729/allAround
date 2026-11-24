//! Android runtime: one engine thread owning an `ALooper`, the sensor event
//! queue and the [`facetilt_core::Engine`]; one camera worker thread (see
//! [`camera`]). All cross-thread communication uses channels plus
//! `ALooper_wake`, so the engine thread sleeps in `ALooper_pollOnce` between
//! events: no polling loops, no wake locks, no timers while idle.
//!
//! ```text
//!  Kotlin ──JNI──► control channel ──┐
//!  ASensorEventQueue (fd) ───────────┼──► engine thread (ALooper) ──► Engine::handle ──► commands
//!  camera worker / image callback ───┘        │                                  │
//!                                              └── DecisionListener (JNI) ◄───────┘ Apply
//! ```

mod camera;
mod clock;
mod sensors;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use facetilt_core::{Command, ControlAction, Engine, Event, StatsSnapshot};
use facetilt_decision::{Decision, Rotation};
use facetilt_vision::Classifier;
use jni::objects::{GlobalRef, JValue};
use jni::JavaVM;
use ndk::looper::{ForeignLooper, ThreadLooper};

use crate::config::BridgeConfig;
use camera::CameraController;
use sensors::Sensors;

/// Looper ident for the sensor event queue.
const IDENT_SENSORS: i32 = 1;

enum Control {
    Stop,
    SetConfig(Box<BridgeConfig>),
    DisplayRotation(Rotation),
    ReportApplied(u32),
}

/// Sends `(timestamp_ms, event)` to the engine thread and wakes its looper.
#[derive(Clone)]
pub(crate) struct Mailbox {
    tx: Sender<(u64, Event)>,
    looper: ForeignLooper,
}

impl Mailbox {
    pub(crate) fn post(&self, ev: Event) {
        // A closed channel means the engine is shutting down: dropping is correct.
        let _ = self.tx.send((clock::now_ms(), ev));
        self.looper.wake();
    }
}

struct Runtime {
    ctl: Sender<Control>,
    looper: ForeignLooper,
    thread: JoinHandle<()>,
}

struct Shared {
    stats: Mutex<[i64; StatsSnapshot::LEN]>,
    camera_id: Mutex<Option<String>>,
}

fn shared() -> &'static Shared {
    static S: OnceLock<Shared> = OnceLock::new();
    S.get_or_init(|| Shared { stats: Mutex::new(StatsSnapshot::default().to_array()), camera_id: Mutex::new(None) })
}

fn runtime() -> MutexGuard<'static, Option<Runtime>> {
    static R: Mutex<Option<Runtime>> = Mutex::new(None);
    // A poisoned lock only means an earlier call panicked; the Option inside is
    // still structurally valid, so recover it.
    R.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn init_logging() {
    static ONCE: AtomicBool = AtomicBool::new(false);
    if !ONCE.swap(true, Ordering::SeqCst) {
        android_logger::init_once(
            android_logger::Config::default().with_tag("FaceTilt").with_max_level(log::LevelFilter::Debug),
        );
    }
}

fn classifier() -> Option<Arc<Classifier>> {
    static C: OnceLock<Option<Arc<Classifier>>> = OnceLock::new();
    C.get_or_init(|| match Classifier::load_embedded() {
        Ok(c) => Some(Arc::new(c)),
        Err(e) => {
            log::error!("could not load classifier, face-aware rotation disabled: {e}");
            None
        }
    })
    .clone()
}

pub fn start(vm: JavaVM, listener: GlobalRef, package: &str, rotation: Rotation, cfg: BridgeConfig) -> bool {
    init_logging();
    stop();
    let (ctl_tx, ctl_rx) = mpsc::channel::<Control>();
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Option<ForeignLooper>>(1);
    let package = package.to_owned();
    let spawn = std::thread::Builder::new().name("facetilt-engine".into()).spawn(move || {
        crate::guard::ffi_guard("engine thread", (), || {
            engine_thread(vm, listener, package, rotation, cfg, ctl_rx, ready_tx);
        });
    });
    let Ok(thread) = spawn else {
        log::error!("could not spawn engine thread");
        return false;
    };
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Some(looper)) => {
            *runtime() = Some(Runtime { ctl: ctl_tx, looper, thread });
            true
        }
        _ => {
            let _ = thread.join();
            false
        }
    }
}

pub fn stop() {
    let rt = runtime().take();
    if let Some(rt) = rt {
        let _ = rt.ctl.send(Control::Stop);
        rt.looper.wake();
        if rt.thread.join().is_err() {
            log::error!("engine thread panicked during shutdown");
        }
    }
}

fn send(c: Control) {
    if let Some(rt) = runtime().as_ref() {
        let _ = rt.ctl.send(c);
        rt.looper.wake();
    }
}

pub fn set_config(cfg: BridgeConfig) {
    send(Control::SetConfig(Box::new(cfg)));
}

pub fn set_display_rotation(r: Rotation) {
    send(Control::DisplayRotation(r));
}

pub fn report_applied(us: u32) {
    send(Control::ReportApplied(us));
}

pub fn stats() -> [i64; StatsSnapshot::LEN] {
    *lock(&shared().stats)
}

pub fn front_camera_id() -> Option<String> {
    lock(&shared().camera_id).clone()
}

fn engine_thread(
    vm: JavaVM,
    listener: GlobalRef,
    package: String,
    rotation: Rotation,
    mut cfg: BridgeConfig,
    ctl_rx: Receiver<Control>,
    ready_tx: mpsc::SyncSender<Option<ForeignLooper>>,
) {
    let looper = ThreadLooper::prepare();
    let mut sensors = match Sensors::new(&package, &looper, IDENT_SENSORS) {
        Ok(s) => s,
        Err(e) => {
            log::error!("sensors unavailable: {e}");
            let _ = ready_tx.send(None);
            return;
        }
    };
    let mut env = match vm.attach_current_thread_permanently() {
        Ok(env) => env,
        Err(e) => {
            log::error!("cannot attach engine thread to the JVM: {e}");
            let _ = ready_tx.send(None);
            return;
        }
    };
    let (ev_tx, ev_rx) = mpsc::channel::<(u64, Event)>();
    let mailbox = Mailbox { tx: ev_tx, looper: looper.as_foreign().clone() };
    let mut camera = CameraController::start(mailbox, classifier(), cfg.sensor_orientation_override);
    *lock(&shared().camera_id) = camera.camera_id();
    let mut engine = Engine::new(cfg.engine, rotation, clock::now_ms());
    let _ = ready_tx.send(Some(looper.as_foreign().clone()));
    log::info!("engine started (sensor: {}, camera: {:?})", sensors.describe(), camera.camera_id());

    let mut evaluations_seen = u64::MAX;
    'run: loop {
        let timeout = engine.next_deadline().map(|d| Duration::from_millis(d.saturating_sub(clock::now_ms())));
        let polled = match timeout {
            Some(t) => looper.poll_once_timeout(t),
            None => looper.poll_once(),
        };
        if let Err(e) = polled {
            log::error!("looper error: {e:?}");
            break;
        }
        // 1. Control messages from Kotlin.
        while let Ok(c) = ctl_rx.try_recv() {
            match c {
                Control::Stop => break 'run,
                Control::SetConfig(c) => {
                    cfg = *c;
                    engine.set_config(cfg.engine);
                    camera.set_orientation_override(cfg.sensor_orientation_override);
                }
                Control::DisplayRotation(r) => {
                    let cmds = engine.handle(clock::now_ms(), Event::DisplayRotation(r));
                    execute(&mut env, &listener, &mut camera, cmds);
                }
                Control::ReportApplied(us) => {
                    engine.handle(clock::now_ms(), Event::ApplyReported { apply_us: us });
                }
            }
        }
        // 2. Sensor events (timestamps are CLOCK_BOOTTIME, like clock::now_ms).
        let trace = cfg.debug_trace;
        sensors.drain(|t_ms, ev| {
            if trace {
                if let Event::Gravity { v } = ev {
                    log::debug!(target: "FaceTiltTrace", "{t_ms},{:.4},{:.4},{:.4}", v[0], v[1], v[2]);
                }
            }
            let cmds = engine.handle(t_ms, ev);
            execute(&mut env, &listener, &mut camera, cmds);
        });
        // 3. Camera / classifier events.
        while let Ok((t_ms, ev)) = ev_rx.try_recv() {
            let cmds = engine.handle(t_ms, ev);
            execute(&mut env, &listener, &mut camera, cmds);
        }
        // 4. Deadlines (settle timeout, capture timeout).
        let now = clock::now_ms();
        if engine.next_deadline().is_some_and(|d| now >= d) {
            let cmds = engine.handle(now, Event::Tick);
            execute(&mut env, &listener, &mut camera, cmds);
        }
        // 5. Publish stats when something changed.
        let snap = engine.stats(now);
        if snap.evaluations != evaluations_seen || engine.camera_wanted() {
            evaluations_seen = snap.evaluations;
            *lock(&shared().stats) = snap.to_array();
        }
    }

    // Shutdown: discard any in-flight capture, close the camera, unregister sensors.
    let cmds = engine.handle(clock::now_ms(), Event::Stop);
    execute(&mut env, &listener, &mut camera, cmds);
    *lock(&shared().stats) = engine.stats(clock::now_ms()).to_array();
    camera.shutdown();
    drop(sensors);
    log::info!("engine stopped");
}

fn execute(
    env: &mut jni::JNIEnv<'_>,
    listener: &GlobalRef,
    camera: &mut CameraController,
    cmds: facetilt_core::Commands,
) {
    for c in cmds.iter() {
        match c {
            Command::OpenCamera { session } => camera.open(session),
            Command::CaptureFrame { session } => camera.capture(session),
            Command::CloseCamera { session } => camera.close(session),
            Command::Apply(d) => notify(env, listener, d),
            Command::Control(a) => notify_control(env, listener, a),
        }
    }
}

/// Assist mode: tell Kotlin to take rotation control from the system (`Seize`)
/// or hand it back (`Release`).
fn notify_control(env: &mut jni::JNIEnv<'_>, listener: &GlobalRef, action: ControlAction) {
    let seize = action == ControlAction::Seize;
    if env.call_method(listener, "onAssistControl", "(Z)V", &[JValue::Bool(u8::from(seize))]).is_err() {
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        log::error!("onAssistControl callback failed");
    }
}

fn notify(env: &mut jni::JNIEnv<'_>, listener: &GlobalRef, d: Decision) {
    let args = [JValue::Int(i32::from(d.rotation.index())), JValue::Int(d.reason as i32)];
    if env.call_method(listener, "onOrientationDecision", "(II)V", &args).is_err() {
        // Never leave a pending Java exception on this thread.
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_describe();
            let _ = env.exception_clear();
        }
        log::error!("onOrientationDecision callback failed");
    }
}
