//! Int8 head-orientation classifier running on tract (pure Rust, single thread).
//!
//! The model (`model/facetilt_orient_int8.onnx`, < 100 KB) is embedded in the
//! library, so no file is ever read at runtime. It outputs 5 logits in
//! [`HeadClass`] order; softmax is computed here.
//!
//! Privacy: [`Classifier::classify`] zeroes the caller's thumbnail and the
//! normalised input tensor before returning (volatile writes via `zeroize`).
//! Intermediate CNN activations are transient tract allocations (abstract
//! feature maps ≤ 48×48), freed when the call returns.

use std::fmt;
use std::sync::Arc;

use facetilt_decision::{HeadClass, HeadObservation};
use tract_onnx::prelude::*;
use zeroize::Zeroize;

use crate::preprocess::{softmax, standardize};
use crate::{Thumb, PIXELS, SIDE};

/// The embedded int8 QDQ ONNX model.
pub static MODEL_BYTES: &[u8] = include_bytes!("../model/facetilt_orient_int8.onnx");

/// Loading or inference failure.
#[derive(Debug)]
pub struct ClassifierError(String);

impl fmt::Display for ClassifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "classifier: {}", self.0)
    }
}

impl std::error::Error for ClassifierError {}

impl From<TractError> for ClassifierError {
    fn from(e: TractError) -> Self {
        ClassifierError(format!("{e:#}"))
    }
}

/// Result of one classification, including the raw probabilities (used by tests).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prediction {
    /// Arg-max class and its probability.
    pub observation: HeadObservation,
    /// All class probabilities.
    pub probs: [f32; HeadClass::COUNT],
}

/// Loaded, optimised model. `Send + Sync`; `classify` takes `&self`.
pub struct Classifier {
    plan: Arc<TypedSimplePlan>,
}

impl fmt::Debug for Classifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Classifier").finish_non_exhaustive()
    }
}

impl Classifier {
    /// Loads the embedded model.
    pub fn load_embedded() -> Result<Self, ClassifierError> {
        Self::load(MODEL_BYTES)
    }

    /// Loads a model from bytes (tests use this to compare float/int8 models).
    pub fn load(bytes: &[u8]) -> Result<Self, ClassifierError> {
        let model = tract_onnx::onnx()
            .model_for_read(&mut std::io::Cursor::new(bytes))?
            .with_input_fact(0, f32::fact([1, 1, SIDE, SIDE]).into())?
            .into_optimized()?;
        let plan = model.into_runnable()?;
        Ok(Classifier { plan })
    }

    /// Classifies `thumb` (mirror view, see [`crate::orient`]) and **zeroes it**.
    ///
    /// Returns `Ok(None)` for unusable (near-uniform) frames.
    pub fn classify(&self, thumb: &mut Thumb) -> Result<Option<Prediction>, ClassifierError> {
        let result = self.classify_inner(thumb);
        thumb.zeroize();
        result
    }

    fn classify_inner(&self, thumb: &Thumb) -> Result<Option<Prediction>, ClassifierError> {
        let mut norm = [0.0f32; PIXELS];
        if !standardize(thumb, &mut norm) {
            return Ok(None);
        }
        let tensor = Tensor::from_shape::<f32>(&[1, 1, SIDE, SIDE], &norm);
        norm.zeroize();
        let mut input = Arc::new(tensor?);
        let run = self.plan.run(tvec!(TValue::from_const(Arc::clone(&input))));
        // Zero the normalised image inside the tensor whatever the outcome.
        // tract has dropped its references once `run` returns, so `get_mut`
        // succeeds; if it ever did not, the tensor is still freed right below.
        if let Some(t) = Arc::get_mut(&mut input) {
            if let Ok(mut view) = t.try_as_plain_ram_mut() {
                if let Ok(s) = view.as_slice_mut::<f32>() {
                    s.zeroize();
                }
            }
        }
        drop(input);
        let outputs = run?;
        let out = outputs.first().ok_or_else(|| ClassifierError("no output".into()))?;
        let view = out.try_as_plain_ram()?;
        let logits_slice = view.as_slice::<f32>()?;
        let logits: [f32; HeadClass::COUNT] =
            logits_slice.try_into().map_err(|_| ClassifierError(format!("{} logits", logits_slice.len())))?;
        let probs = softmax(&logits);
        let (best, &confidence) = probs
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .ok_or_else(|| ClassifierError("empty output".into()))?;
        let class = HeadClass::from_index(best).ok_or_else(|| ClassifierError("class index".into()))?;
        Ok(Some(Prediction { observation: HeadObservation { class, confidence }, probs }))
    }

    /// Raw logits for a standardised input (golden tests only).
    #[doc(hidden)]
    pub fn logits_for_test(&self, norm: &[f32; PIXELS]) -> Result<[f32; HeadClass::COUNT], ClassifierError> {
        let t = Tensor::from_shape::<f32>(&[1, 1, SIDE, SIDE], norm)?;
        let outputs = self.plan.run(tvec!(t.into()))?;
        let out = outputs.first().ok_or_else(|| ClassifierError("no output".into()))?;
        let view = out.try_as_plain_ram()?;
        let s = view.as_slice::<f32>()?;
        s.try_into().map_err(|_| ClassifierError("logits".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_is_small() {
        assert!(MODEL_BYTES.len() < 100 * 1024, "model is {} bytes", MODEL_BYTES.len());
    }

    #[test]
    fn loads_and_zeroes_thumbnail() {
        let c = Classifier::load_embedded().unwrap_or_else(|e| panic!("{e}"));
        let mut t = [0u8; PIXELS];
        for (i, p) in t.iter_mut().enumerate() {
            *p = ((i * 37) % 251) as u8;
        }
        let p = c.classify(&mut t).unwrap_or_else(|e| panic!("{e}"));
        assert!(p.is_some());
        assert!(t.iter().all(|&v| v == 0), "thumbnail not zeroed");
        let p = p.map(|p| p.probs.iter().sum::<f32>()).unwrap_or(0.0);
        assert!((p - 1.0).abs() < 1e-4);
    }

    #[test]
    fn uniform_frame_is_unusable() {
        let c = Classifier::load_embedded().unwrap_or_else(|e| panic!("{e}"));
        let mut t = [3u8; PIXELS];
        assert_eq!(c.classify(&mut t).ok().flatten(), None);
        assert!(t.iter().all(|&v| v == 0));
    }
}
