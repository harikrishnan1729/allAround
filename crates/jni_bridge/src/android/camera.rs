//! NDK Camera2 capture (`ACameraManager` / `ACameraDevice` / `AImageReader`).
//!
//! * One worker thread performs the blocking calls (open, configure, close) so
//!   the engine thread never stalls on the camera service.
//! * The stream is the smallest supported `YUV_420_888` size (preferring a
//!   short side ≥ 128 px) feeding an `AImageReader` only — **no preview
//!   surface**. The system camera indicator still shows: we never try to hide it.
//! * Frames that arrive before the engine asks for one are released unread.
//! * The requested frame is processed **inside `onImageAvailable`**: the Y plane
//!   is box-filtered in place into a 2,304-byte stack buffer, every plane of the
//!   camera buffer is zeroed (when the reader was created with CPU-write usage),
//!   the `AImage` is released, and only then does inference run on the 48×48
//!   thumbnail. Only the resulting class + confidence leaves this module.

use std::ffi::{c_void, CStr, CString};
use std::ptr::{self, NonNull};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Instant;

use facetilt_core::Event;
use facetilt_vision::{box_downsample, to_mirror_view, Classifier, Thumb, PIXELS};
use ndk_sys as ffi;
use zeroize::Zeroize;

use super::Mailbox;
use crate::guard::ffi_guard;

/// ATrace section for Perfetto (no-op unless the app is being traced).
fn trace_section(name: &str) -> Option<ndk::trace::Section> {
    ndk::trace::Section::new(name).ok()
}

const MAX_IMAGES: i32 = 2; // acquireLatestImage needs >= 2
const PREFERRED_MIN_SIDE: i32 = 128;

enum CamCmd {
    Open(u32),
    Close(u32),
    SetOverride(Option<i32>),
    Shutdown,
}

/// State shared with NDK callbacks (which run on camera / reader threads).
struct FrameShared {
    mailbox: Mailbox,
    worker: Sender<CamCmd>,
    classifier: Option<Arc<Classifier>>,
    /// Session whose next frame must be classified (0 = none).
    capture: AtomicU32,
    /// Session whose reader is live (0 = none).
    active: AtomicU32,
    /// The current reader's buffers are CPU-writable, so they can be zeroed.
    writable: AtomicBool,
    orientation: AtomicI32,
    mirror: AtomicBool,
}

#[derive(Clone)]
struct CameraInfo {
    id: CString,
    orientation: i32,
    front: bool,
    width: i32,
    height: i32,
}

pub struct CameraController {
    tx: Sender<CamCmd>,
    worker: Option<JoinHandle<()>>,
    shared: Arc<FrameShared>,
    camera_id: Option<String>,
}

impl CameraController {
    pub fn start(mailbox: Mailbox, classifier: Option<Arc<Classifier>>, orientation_override: Option<i32>) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(FrameShared {
            mailbox,
            worker: tx.clone(),
            classifier,
            capture: AtomicU32::new(0),
            active: AtomicU32::new(0),
            writable: AtomicBool::new(false),
            orientation: AtomicI32::new(0),
            mirror: AtomicBool::new(true),
        });
        let (info_tx, info_rx) = mpsc::sync_channel::<Option<String>>(1);
        let worker_shared = Arc::clone(&shared);
        let worker = std::thread::Builder::new()
            .name("facetilt-camera".into())
            .spawn(move || {
                ffi_guard("camera worker", (), || worker_main(&worker_shared, &rx, &info_tx, orientation_override));
            })
            .ok();
        let camera_id = if worker.is_some() { info_rx.recv().ok().flatten() } else { None };
        CameraController { tx, worker, shared, camera_id }
    }

    pub fn camera_id(&self) -> Option<String> {
        self.camera_id.clone()
    }

    pub fn open(&mut self, session: u32) {
        if self.worker.is_none() || self.tx.send(CamCmd::Open(session)).is_err() {
            self.shared.mailbox.post(Event::CameraError { session });
        }
    }

    /// Arms capture of the next frame. Engine sends this only after
    /// `CameraOpened`, i.e. once `active == session`.
    pub fn capture(&mut self, session: u32) {
        if self.shared.active.load(Ordering::Acquire) == session {
            self.shared.capture.store(session, Ordering::Release);
        }
    }

    pub fn close(&mut self, session: u32) {
        // Disarm immediately so a frame racing with the close is not classified.
        let _ = self.shared.capture.compare_exchange(session, 0, Ordering::AcqRel, Ordering::Acquire);
        let _ = self.tx.send(CamCmd::Close(session));
    }

    pub fn set_orientation_override(&mut self, o: Option<i32>) {
        let _ = self.tx.send(CamCmd::SetOverride(o));
    }

    pub fn shutdown(&mut self) {
        self.shared.capture.store(0, Ordering::Release);
        let _ = self.tx.send(CamCmd::Shutdown);
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

impl Drop for CameraController {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ----------------------------------------------------------------------------- worker

struct Manager(NonNull<ffi::ACameraManager>);

impl Drop for Manager {
    fn drop(&mut self) {
        // SAFETY: created by ACameraManager_create, deleted exactly once.
        unsafe { ffi::ACameraManager_delete(self.0.as_ptr()) }
    }
}

/// 0 = unknown, 1 = CPU-write usage works, 2 = it does not on this device.
static WRITABLE_SUPPORT: AtomicU8 = AtomicU8::new(0);

fn worker_main(
    shared: &Arc<FrameShared>,
    rx: &Receiver<CamCmd>,
    info_tx: &mpsc::SyncSender<Option<String>>,
    orientation_override: Option<i32>,
) {
    // SAFETY: plain constructor; null on failure.
    let Some(mgr) = NonNull::new(unsafe { ffi::ACameraManager_create() }).map(Manager) else {
        let _ = info_tx.send(None);
        return;
    };
    let info = select_front_camera(&mgr);
    if let Some(i) = &info {
        shared.orientation.store(orientation_override.unwrap_or(i.orientation), Ordering::Release);
        shared.mirror.store(i.front, Ordering::Release);
        log::info!("front camera {:?}: orientation {}, stream {}x{}", i.id, i.orientation, i.width, i.height);
    } else {
        log::warn!("no usable front camera: face-aware rotation will fall back to gravity");
    }
    let _ = info_tx.send(info.as_ref().map(|i| i.id.to_string_lossy().into_owned()));

    let mut open: Option<OpenCamera> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            CamCmd::Open(session) => {
                let _t = trace_section("facetilt:camera_open");
                drop(open.take());
                let Some(i) = info.as_ref() else {
                    shared.mailbox.post(Event::CameraError { session });
                    continue;
                };
                match open_with_fallback(&mgr, i, session, shared) {
                    Ok(cam) => {
                        shared.active.store(session, Ordering::Release);
                        open = Some(cam);
                        shared.mailbox.post(Event::CameraOpened { session });
                    }
                    Err(e) => {
                        // Denied, in use by another app, disabled by policy or by
                        // the privacy toggle, … → the engine falls back to gravity.
                        log::info!("camera open failed: {e}");
                        shared.mailbox.post(Event::CameraError { session });
                    }
                }
            }
            CamCmd::Close(session) => {
                let _t = trace_section("facetilt:camera_close");
                if open.as_ref().is_some_and(|o| o.session == session) {
                    drop(open.take());
                }
            }
            CamCmd::SetOverride(o) => {
                if let Some(i) = &info {
                    shared.orientation.store(o.unwrap_or(i.orientation), Ordering::Release);
                }
            }
            CamCmd::Shutdown => break,
        }
    }
    drop(open.take());
    drop(mgr);
}

fn open_with_fallback(
    mgr: &Manager,
    info: &CameraInfo,
    session: u32,
    shared: &Arc<FrameShared>,
) -> Result<OpenCamera, String> {
    if WRITABLE_SUPPORT.load(Ordering::Relaxed) != 2 {
        match OpenCamera::open(mgr, info, session, shared, true) {
            Ok(c) => {
                WRITABLE_SUPPORT.store(1, Ordering::Relaxed);
                return Ok(c);
            }
            Err(OpenError::Configure(e)) if WRITABLE_SUPPORT.load(Ordering::Relaxed) == 0 => {
                log::info!("CPU-writable camera buffers rejected ({e}); using read-only buffers");
                WRITABLE_SUPPORT.store(2, Ordering::Relaxed);
            }
            Err(e) => return Err(e.to_string()),
        }
    }
    OpenCamera::open(mgr, info, session, shared, false).map_err(|e| e.to_string())
}

// ----------------------------------------------------------------------------- camera selection

/// Reads a metadata entry; `None` if absent.
fn const_entry(
    meta: *const ffi::ACameraMetadata,
    tag: ffi::acamera_metadata_tag,
) -> Option<ffi::ACameraMetadata_const_entry> {
    // SAFETY: zeroed is a valid bit pattern for this POD struct (null pointers, 0 counts).
    let mut e: ffi::ACameraMetadata_const_entry = unsafe { std::mem::zeroed() };
    // SAFETY: meta is a valid characteristics object owned by the caller; e is writable.
    let st = unsafe { ffi::ACameraMetadata_getConstEntry(meta, tag.0, &mut e) };
    (st == ffi::camera_status_t::ACAMERA_OK && e.count > 0).then_some(e)
}

fn select_front_camera(mgr: &Manager) -> Option<CameraInfo> {
    let mut list: *mut ffi::ACameraIdList = ptr::null_mut();
    // SAFETY: mgr valid; list receives an owned id list freed below.
    let st = unsafe { ffi::ACameraManager_getCameraIdList(mgr.0.as_ptr(), &mut list) };
    if st != ffi::camera_status_t::ACAMERA_OK || list.is_null() {
        return None;
    }
    let mut found = None;
    // SAFETY: list is valid until deleteCameraIdList; cameraIds has numCameras entries.
    let (n, ids) = unsafe { ((*list).numCameras.max(0) as usize, (*list).cameraIds) };
    for i in 0..n {
        // SAFETY: i < numCameras; each entry is a NUL-terminated string owned by the list.
        let id = unsafe { CStr::from_ptr(*ids.add(i)) };
        let mut meta: *mut ffi::ACameraMetadata = ptr::null_mut();
        // SAFETY: valid manager and id; meta is freed below.
        let st = unsafe { ffi::ACameraManager_getCameraCharacteristics(mgr.0.as_ptr(), id.as_ptr(), &mut meta) };
        if st != ffi::camera_status_t::ACAMERA_OK || meta.is_null() {
            continue;
        }
        let front = const_entry(meta, ffi::acamera_metadata_tag::ACAMERA_LENS_FACING)
            // SAFETY: LENS_FACING is a u8 entry with count >= 1 (checked in const_entry).
            .map(|e| unsafe { *e.data.u8_ } as u32 == ffi::acamera_metadata_enum_acamera_lens_facing::ACAMERA_LENS_FACING_FRONT.0)
            .unwrap_or(false);
        if front {
            let orientation = const_entry(meta, ffi::acamera_metadata_tag::ACAMERA_SENSOR_ORIENTATION)
                // SAFETY: SENSOR_ORIENTATION is an i32 entry with count >= 1.
                .map(|e| unsafe { *e.data.i32_ })
                .unwrap_or(270);
            if let Some((width, height)) = smallest_yuv_size(meta) {
                found = Some(CameraInfo { id: id.to_owned(), orientation, front, width, height });
            }
        }
        // SAFETY: meta obtained above, freed exactly once.
        unsafe { ffi::ACameraMetadata_free(meta) };
        if found.is_some() {
            break;
        }
    }
    // SAFETY: list obtained above, freed exactly once.
    unsafe { ffi::ACameraManager_deleteCameraIdList(list) };
    found
}

fn smallest_yuv_size(meta: *const ffi::ACameraMetadata) -> Option<(i32, i32)> {
    let e = const_entry(meta, ffi::acamera_metadata_tag::ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS)?;
    // SAFETY: this tag is an i32 array of `count` elements, valid while `meta` lives.
    let v = unsafe { std::slice::from_raw_parts(e.data.i32_, e.count as usize) };
    let yuv = ffi::AIMAGE_FORMATS::AIMAGE_FORMAT_YUV_420_888.0 as i32;
    let output = ffi::acamera_metadata_enum_acamera_scaler_available_stream_configurations::ACAMERA_SCALER_AVAILABLE_STREAM_CONFIGURATIONS_OUTPUT.0 as i32;
    let sizes = v.chunks_exact(4).filter(|c| c[0] == yuv && c[3] == output).map(|c| (c[1], c[2]));
    let area = |s: &(i32, i32)| i64::from(s.0) * i64::from(s.1);
    let preferred = sizes.clone().filter(|s| s.0.min(s.1) >= PREFERRED_MIN_SIDE).min_by_key(area);
    preferred.or_else(|| sizes.filter(|s| s.0.min(s.1) >= 32).max_by_key(area))
}

// ----------------------------------------------------------------------------- open camera (RAII)

#[derive(Debug)]
enum OpenError {
    Reader(i32),
    Device(i32),
    Configure(String),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::Reader(s) => write!(f, "image reader error {s}"),
            OpenError::Device(s) => write!(f, "openCamera error {s}"),
            OpenError::Configure(s) => write!(f, "session configuration failed: {s}"),
        }
    }
}

/// Everything that exists while the camera is open. Dropping it tears the
/// camera down in the order used by the NDK samples.
struct OpenCamera {
    session: u32,
    shared: Arc<FrameShared>,
    reader: *mut ffi::AImageReader,
    device: *mut ffi::ACameraDevice,
    output: *mut ffi::ACaptureSessionOutput,
    container: *mut ffi::ACaptureSessionOutputContainer,
    target: *mut ffi::ACameraOutputTarget,
    request: *mut ffi::ACaptureRequest,
    capture_session: *mut ffi::ACameraCaptureSession,
    // Callback structs are kept alive for the objects' lifetime (the NDK copies
    // them today, but nothing documents that).
    _listener: Box<ffi::AImageReader_ImageListener>,
    _device_cbs: Box<ffi::ACameraDevice_StateCallbacks>,
    _session_cbs: Box<ffi::ACameraCaptureSession_stateCallbacks>,
}

fn ctx(shared: &Arc<FrameShared>) -> *mut c_void {
    Arc::as_ptr(shared).cast_mut().cast()
}

impl OpenCamera {
    fn open(
        mgr: &Manager,
        info: &CameraInfo,
        session: u32,
        shared: &Arc<FrameShared>,
        writable: bool,
    ) -> Result<Self, OpenError> {
        let mut cam = OpenCamera {
            session,
            shared: Arc::clone(shared),
            reader: ptr::null_mut(),
            device: ptr::null_mut(),
            output: ptr::null_mut(),
            container: ptr::null_mut(),
            target: ptr::null_mut(),
            request: ptr::null_mut(),
            capture_session: ptr::null_mut(),
            _listener: Box::new(ffi::AImageReader_ImageListener {
                context: ctx(shared),
                onImageAvailable: Some(on_image_available),
            }),
            _device_cbs: Box::new(ffi::ACameraDevice_StateCallbacks {
                context: ctx(shared),
                onDisconnected: Some(on_device_disconnected),
                onError: Some(on_device_error),
            }),
            _session_cbs: Box::new(ffi::ACameraCaptureSession_stateCallbacks {
                context: ptr::null_mut(),
                onClosed: Some(on_session_state),
                onReady: Some(on_session_state),
                onActive: Some(on_session_state),
            }),
        };
        shared.writable.store(writable, Ordering::Release);
        let fmt = ffi::AIMAGE_FORMATS::AIMAGE_FORMAT_YUV_420_888.0 as i32;
        let usage = ffi::AHardwareBuffer_UsageFlags::AHARDWAREBUFFER_USAGE_CPU_READ_OFTEN.0
            | if writable { ffi::AHardwareBuffer_UsageFlags::AHARDWAREBUFFER_USAGE_CPU_WRITE_OFTEN.0 } else { 0 };
        // SAFETY: out-pointer is a field of `cam`; on success `cam` owns the reader.
        let st =
            unsafe { ffi::AImageReader_newWithUsage(info.width, info.height, fmt, usage, MAX_IMAGES, &mut cam.reader) };
        if st != ffi::media_status_t::AMEDIA_OK || cam.reader.is_null() {
            return Err(OpenError::Reader(st.0));
        }
        // SAFETY: reader valid; listener box outlives the reader (dropped after delete).
        let st = unsafe { ffi::AImageReader_setImageListener(cam.reader, &mut *cam._listener) };
        if st != ffi::media_status_t::AMEDIA_OK {
            return Err(OpenError::Reader(st.0));
        }
        let mut window: *mut ffi::ANativeWindow = ptr::null_mut();
        // SAFETY: reader valid; the window is owned by the reader (not released by us).
        let st = unsafe { ffi::AImageReader_getWindow(cam.reader, &mut window) };
        if st != ffi::media_status_t::AMEDIA_OK || window.is_null() {
            return Err(OpenError::Reader(st.0));
        }
        // SAFETY: manager and id valid; callbacks box outlives the device.
        let st = unsafe {
            ffi::ACameraManager_openCamera(mgr.0.as_ptr(), info.id.as_ptr(), &mut *cam._device_cbs, &mut cam.device)
        };
        if st != ffi::camera_status_t::ACAMERA_OK || cam.device.is_null() {
            return Err(OpenError::Device(st.0));
        }
        let cfg = |what: &str, st: ffi::camera_status_t| -> Result<(), OpenError> {
            if st == ffi::camera_status_t::ACAMERA_OK {
                Ok(())
            } else {
                Err(OpenError::Configure(format!("{what}: {}", st.0)))
            }
        };
        // SAFETY: (whole block) every pointer passed in was created above and is
        // owned by `cam`; every out-pointer is a field of `cam`, so partial
        // failures are cleaned up by Drop.
        unsafe {
            cfg("output", ffi::ACaptureSessionOutput_create(window, &mut cam.output))?;
            cfg("container", ffi::ACaptureSessionOutputContainer_create(&mut cam.container))?;
            cfg("container add", ffi::ACaptureSessionOutputContainer_add(cam.container, cam.output))?;
            cfg("target", ffi::ACameraOutputTarget_create(window, &mut cam.target))?;
            cfg(
                "request",
                ffi::ACameraDevice_createCaptureRequest(
                    cam.device,
                    ffi::ACameraDevice_request_template::TEMPLATE_PREVIEW,
                    &mut cam.request,
                ),
            )?;
            cfg("add target", ffi::ACaptureRequest_addTarget(cam.request, cam.target))?;
            cfg(
                "session",
                ffi::ACameraDevice_createCaptureSession(
                    cam.device,
                    cam.container,
                    &*cam._session_cbs,
                    &mut cam.capture_session,
                ),
            )?;
            // Repeating request so auto-exposure converges while the tilt is
            // still in progress; frames are dropped unread until requested.
            let mut req = cam.request;
            cfg(
                "repeating",
                ffi::ACameraCaptureSession_setRepeatingRequest(
                    cam.capture_session,
                    ptr::null_mut(),
                    1,
                    &mut req,
                    ptr::null_mut(),
                ),
            )?;
        }
        Ok(cam)
    }
}

impl Drop for OpenCamera {
    fn drop(&mut self) {
        self.shared.active.store(0, Ordering::Release);
        self.shared.capture.store(0, Ordering::Release);
        // SAFETY: each handle is either null or a live object created in `open`
        // and owned exclusively by `self`; each is released exactly once, in
        // the NDK samples' order. AImageReader_delete stops (joins) the reader's
        // callback thread, so no image callback runs after it returns.
        unsafe {
            if !self.reader.is_null() {
                ffi::AImageReader_setImageListener(self.reader, ptr::null_mut());
            }
            if !self.capture_session.is_null() {
                ffi::ACameraCaptureSession_stopRepeating(self.capture_session);
                ffi::ACameraCaptureSession_close(self.capture_session);
            }
            if !self.request.is_null() {
                ffi::ACaptureRequest_free(self.request);
            }
            if !self.target.is_null() {
                ffi::ACameraOutputTarget_free(self.target);
            }
            if !self.container.is_null() {
                ffi::ACaptureSessionOutputContainer_free(self.container);
            }
            if !self.output.is_null() {
                ffi::ACaptureSessionOutput_free(self.output);
            }
            if !self.device.is_null() {
                ffi::ACameraDevice_close(self.device);
            }
            if !self.reader.is_null() {
                ffi::AImageReader_delete(self.reader);
            }
        }
    }
}

// ----------------------------------------------------------------------------- callbacks

/// # Safety
/// `context` must be `Arc::as_ptr` of a live `FrameShared`. The `OpenCamera`
/// owning the device/reader holds an `Arc` clone, and the controller holds
/// another until after every `OpenCamera` is dropped, so it outlives the callbacks.
unsafe fn shared_from<'a>(context: *mut c_void) -> Option<&'a FrameShared> {
    // SAFETY: guaranteed by the caller contract above.
    unsafe { context.cast::<FrameShared>().cast_const().as_ref() }
}

unsafe extern "C" fn on_session_state(_context: *mut c_void, _session: *mut ffi::ACameraCaptureSession) {}

fn device_lost(shared: &FrameShared, why: &str) {
    shared.capture.store(0, Ordering::Release);
    let session = shared.active.swap(0, Ordering::AcqRel);
    if session != 0 {
        log::info!("camera {why}; falling back to gravity");
        shared.mailbox.post(Event::CameraError { session });
        let _ = shared.worker.send(CamCmd::Close(session));
    }
}

unsafe extern "C" fn on_device_disconnected(context: *mut c_void, _device: *mut ffi::ACameraDevice) {
    ffi_guard("onDisconnected", (), || {
        // SAFETY: context contract documented on `shared_from`.
        if let Some(s) = unsafe { shared_from(context) } {
            device_lost(s, "disconnected (higher-priority client or privacy toggle)");
        }
    });
}

unsafe extern "C" fn on_device_error(context: *mut c_void, _device: *mut ffi::ACameraDevice, error: i32) {
    ffi_guard("onError", (), || {
        // SAFETY: context contract documented on `shared_from`.
        if let Some(s) = unsafe { shared_from(context) } {
            device_lost(s, &format!("error {error}"));
        }
    });
}

unsafe extern "C" fn on_image_available(context: *mut c_void, reader: *mut ffi::AImageReader) {
    ffi_guard("onImageAvailable", (), || {
        // SAFETY: context contract documented on `shared_from`; `reader` is the
        // live reader this listener is installed on (callback invariant).
        if let Some(s) = unsafe { shared_from(context) } {
            handle_image(s, reader);
        }
    });
}

/// RAII owner of one acquired `AImage`.
struct Image(NonNull<ffi::AImage>);

impl Drop for Image {
    fn drop(&mut self) {
        // SAFETY: acquired from the reader, deleted exactly once (returns the
        // buffer to the camera).
        unsafe { ffi::AImage_delete(self.0.as_ptr()) }
    }
}

impl Image {
    fn plane(&self, idx: i32) -> Option<(*mut u8, usize)> {
        let mut data: *mut u8 = ptr::null_mut();
        let mut len: i32 = 0;
        // SAFETY: image valid; out-pointers are locals.
        let st = unsafe { ffi::AImage_getPlaneData(self.0.as_ptr(), idx, &mut data, &mut len) };
        (st == ffi::media_status_t::AMEDIA_OK && !data.is_null() && len > 0).then_some((data, len as usize))
    }

    fn i32_prop(&self, f: unsafe extern "C" fn(*const ffi::AImage, *mut i32) -> ffi::media_status_t) -> Option<i32> {
        let mut v = 0;
        // SAFETY: image valid; `f` is one of the AImage_get* accessors taking (image, out).
        let st = unsafe { f(self.0.as_ptr(), &mut v) };
        (st == ffi::media_status_t::AMEDIA_OK).then_some(v)
    }

    /// Box-filters the Y plane (respecting the crop rectangle) into `out`.
    fn downsample_y(&self, out: &mut Thumb) -> Option<()> {
        let width = self.i32_prop(ffi::AImage_getWidth)?;
        let height = self.i32_prop(ffi::AImage_getHeight)?;
        let mut stride = 0;
        let mut pixel_stride = 0;
        // SAFETY: image valid; plane 0 always exists for YUV_420_888.
        unsafe {
            if ffi::AImage_getPlaneRowStride(self.0.as_ptr(), 0, &mut stride) != ffi::media_status_t::AMEDIA_OK
                || ffi::AImage_getPlanePixelStride(self.0.as_ptr(), 0, &mut pixel_stride)
                    != ffi::media_status_t::AMEDIA_OK
            {
                return None;
            }
        }
        if pixel_stride != 1 || stride <= 0 {
            return None; // Y plane is guaranteed pixel stride 1 by the YUV_420_888 contract.
        }
        let mut crop = ffi::AImageCropRect { left: 0, top: 0, right: width, bottom: height };
        // SAFETY: image valid; crop is a local out-param. On failure we keep the full frame.
        unsafe { ffi::AImage_getCropRect(self.0.as_ptr(), &mut crop) };
        let left = crop.left.clamp(0, width) as usize;
        let top = crop.top.clamp(0, height) as usize;
        let w = (crop.right.clamp(0, width) as usize).saturating_sub(left);
        let h = (crop.bottom.clamp(0, height) as usize).saturating_sub(top);
        let (data, len) = self.plane(0)?;
        // SAFETY: AImage_getPlaneData guarantees `len` readable bytes that stay
        // mapped until AImage_delete; we hold the image exclusively and nothing
        // writes to it while this shared slice exists (it ends with this fn).
        let plane = unsafe { std::slice::from_raw_parts(data.cast_const(), len) };
        let start = top * stride as usize + left;
        box_downsample(plane.get(start..)?, w, h, stride as usize, out).ok()
    }

    /// Overwrites every plane of the camera buffer with zeros. Only valid when
    /// the reader was created with `AHARDWAREBUFFER_USAGE_CPU_WRITE_OFTEN`.
    fn zero_planes(&self) {
        let n = self.i32_prop(ffi::AImage_getNumberOfPlanes).unwrap_or(0);
        for i in 0..n {
            if let Some((data, len)) = self.plane(i) {
                // SAFETY: `len` bytes are mapped writable (reader usage includes
                // CPU_WRITE_OFTEN); no other reference to the buffer is alive
                // (the shared slice from `downsample_y` has ended). U and V may
                // overlap; the mutable slices are created one at a time.
                let s = unsafe { std::slice::from_raw_parts_mut(data, len) };
                s.zeroize();
            }
        }
    }
}

fn handle_image(shared: &FrameShared, reader: *mut ffi::AImageReader) {
    let mut raw: *mut ffi::AImage = ptr::null_mut();
    // SAFETY: reader is live (callback invariant); raw receives an owned image.
    let st = unsafe { ffi::AImageReader_acquireLatestImage(reader, &mut raw) };
    let Some(image) = NonNull::new(raw).filter(|_| st == ffi::media_status_t::AMEDIA_OK).map(Image) else {
        return;
    };
    let target = shared.capture.load(Ordering::Acquire);
    if target == 0
        || target != shared.active.load(Ordering::Acquire)
        || shared.capture.compare_exchange(target, 0, Ordering::AcqRel, Ordering::Acquire).is_err()
    {
        // Not requested: release the buffer without reading a single pixel.
        return;
    }
    let frame_section = trace_section("facetilt:frame");
    let mut thumb: Thumb = [0u8; PIXELS];
    let ok = image.downsample_y(&mut thumb).is_some();
    if shared.writable.load(Ordering::Acquire) {
        image.zero_planes();
    }
    drop(image); // release the camera buffer before any inference
    drop(frame_section);

    let mut view: Thumb = [0u8; PIXELS];
    let mut inference_us = 0u32;
    let head = if ok {
        to_mirror_view(
            &thumb,
            shared.orientation.load(Ordering::Acquire),
            shared.mirror.load(Ordering::Acquire),
            &mut view,
        );
        thumb.zeroize();
        let _t = trace_section("facetilt:inference");
        let t0 = Instant::now();
        let r = shared.classifier.as_ref().and_then(|c| c.classify(&mut view).ok().flatten());
        inference_us = u32::try_from(t0.elapsed().as_micros()).unwrap_or(u32::MAX);
        r.map(|p| p.observation)
    } else {
        None
    };
    thumb.zeroize();
    view.zeroize();
    shared.mailbox.post(Event::FrameClassified { session: target, head, inference_us });
}
