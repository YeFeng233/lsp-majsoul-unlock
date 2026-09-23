//! candle CNN inference. Architecture mirrors `train/train.py` **after** its
//! BatchNorm-folding export, so this side is pure Conv1d + Linear (no BN op).
//!
//! Weights are loaded from a `.safetensors` buffer whose keys are:
//! `conv_in.{weight,bias}`, `res.{i}.conv{1,2}.{weight,bias}`,
//! `fc.{weight,bias}`, `head.{weight,bias}`.

use candle_core::{Device, Result, Tensor};
use candle_nn::{conv1d, linear, Conv1d, Conv1dConfig, Linear, Module, VarBuilder};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::Geometry;

/// Backbone width / depth / head — MUST match `train/train.py`.
pub const CONV: usize = 64;
pub const BLOCKS: usize = 3;
pub const FC: usize = 256;

/// Native bridge supplied by the Android host. The callback receives only the
/// encoded observation tensor and returns action logits; legal masking stays Rust-side.
pub type ExternalInferenceFn = unsafe extern "C" fn(u8, *const f32, usize, *mut f32, usize) -> i32;
pub type ExternalActivateFn = unsafe extern "C" fn(u8) -> i32;

pub struct Model {
    conv_in: Conv1d,
    res: Vec<(Conv1d, Conv1d)>,
    fc: Linear,
    head: Linear,
    geo: Geometry,
    external: Option<ExternalInferenceFn>,
    external_failed: AtomicBool,
}

impl Model {
    /// Build the model for `num_players` from a safetensors byte buffer.
    pub fn from_safetensors(bytes: Vec<u8>, num_players: u8) -> Result<Self> {
        let dev = Device::Cpu;
        let vb = VarBuilder::from_buffered_safetensors(bytes, candle_core::DType::F32, &dev)?;
        let geo = Geometry::for_players(num_players);
        let cfg = Conv1dConfig {
            padding: 1,
            ..Default::default()
        };

        let conv_in = conv1d(geo.channels, CONV, 3, cfg, vb.pp("conv_in"))?;
        let mut res = Vec::with_capacity(BLOCKS);
        for i in 0..BLOCKS {
            let b = vb.pp(format!("res.{i}"));
            let c1 = conv1d(CONV, CONV, 3, cfg, b.pp("conv1"))?;
            let c2 = conv1d(CONV, CONV, 3, cfg, b.pp("conv2"))?;
            res.push((c1, c2));
        }
        let fc = linear(CONV * geo.tile_dim, FC, vb.pp("fc"))?;
        let head = linear(FC, geo.action_space, vb.pp("head"))?;

        Ok(Self {
            conv_in,
            res,
            fc,
            head,
            geo,
            external: None,
            external_failed: AtomicBool::new(false),
        })
    }

    /// Construct the bundled model as a safe fallback for an optional local policy callback.
    pub fn from_safetensors_with_external(bytes: Vec<u8>, num_players: u8,
        external: ExternalInferenceFn, activate: ExternalActivateFn) -> Result<Self> {
        let mut model = Self::from_safetensors(bytes, num_players)?;
        let status = unsafe { activate(num_players) };
        model.external = Some(external);
        model.external_failed.store(status != 0, Ordering::Relaxed);
        Ok(model)
    }

    pub fn geometry(&self) -> Geometry {
        self.geo
    }

    /// Forward one flattened `[C*T]` observation to action logits `[A]`.
    pub fn forward_logits(&self, obs: &[f32]) -> Result<Vec<f32>> {
        if let Some(infer) = self.external {
            let mut logits = vec![0.0; self.geo.action_space];
            let status = unsafe {
                infer(self.geo.num_players, obs.as_ptr(), obs.len(), logits.as_mut_ptr(), logits.len())
            };
            if status == 0 && logits.iter().all(|value| value.is_finite()) {
                return Ok(logits);
            }
            self.external_failed.store(true, Ordering::Relaxed);
        }
        let dev = Device::Cpu;
        let x = Tensor::from_vec(
            obs.to_vec(),
            (1, self.geo.channels, self.geo.tile_dim),
            &dev,
        )?;
        let mut x = self.conv_in.forward(&x)?.relu()?;
        for (c1, c2) in &self.res {
            let y = c1.forward(&x)?.relu()?;
            let y = c2.forward(&y)?;
            x = y.add(&x)?.relu()?;
        }
        let x = x.flatten_from(1)?;
        let x = self.fc.forward(&x)?.relu()?;
        let logits = self.head.forward(&x)?;
        logits.squeeze(0)?.to_vec1::<f32>()
    }

    pub fn take_external_failure(&self) -> bool {
        self.external_failed.swap(false, Ordering::Relaxed)
    }

    pub fn uses_external(&self) -> bool { self.external.is_some() }
}
