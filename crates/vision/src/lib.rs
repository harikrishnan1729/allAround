//! # facetilt-vision
//!
//! Everything that touches pixels, in the order the app uses it:
//!
//! 1. [`downsample::box_downsample`] — reads the camera's **Y (luma) plane in
//!    place** and box-filters a centred square into a 48×48 grayscale
//!    thumbnail held in a caller-provided stack buffer ([`Thumb`], 2,304 bytes).
//!    The camera frame is never copied, converted to colour, or stored.
//! 2. [`orient::to_mirror_view`] — rotates/mirrors the thumbnail from sensor
//!    coordinates into the "selfie mirror" view in the device's natural
//!    orientation, the frame the classifier was trained in.
//! 3. [`classifier::Classifier::classify`] — int8 CNN (tract) → head class +
//!    softmax confidence. It zeroes the thumbnail and its own input tensor
//!    before returning.
//!
//! Only a [`facetilt_decision::HeadObservation`] (an enum + one float) leaves
//! this crate. The Python training pipeline (`training/facetilt/boxfilter.py`)
//! implements steps 1–2 bit-for-bit; `tests/parity.rs` checks that against
//! golden files produced by Python.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod classifier;
pub mod downsample;
pub mod orient;
pub mod preprocess;

/// Side length of the classifier input.
pub const SIDE: usize = 48;
/// Number of pixels in a thumbnail.
pub const PIXELS: usize = SIDE * SIDE;
/// A 48×48 8-bit grayscale thumbnail, row-major. Lives on the stack.
pub type Thumb = [u8; PIXELS];

pub use classifier::{Classifier, ClassifierError};
pub use downsample::{box_downsample, DownsampleError};
pub use orient::to_mirror_view;
