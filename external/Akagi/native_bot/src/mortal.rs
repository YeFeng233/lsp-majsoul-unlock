//! Experimental Mortal support; public import/activation accepts four-player v4 only.
//! Other observation/action helpers remain internal compatibility code and do not
//! establish upstream parity. Experimental models are excluded from unattended play.
//!
//! Provides:
//! - Direct PyTorch `.pth` checkpoint loading via `candle_nn::VarBuilder` with
//!   automatic architecture inspection (`in_channels`, `conv_channels`, `num_blocks`,
//!   `version`, `action_space`) and load-time `BatchNorm1d` folding.
//! - Mortal observation feature encoding (`[C, 34]`) for 4P (`1012` v4, `934`
//!   v3, `942` v2, `938` v1) and 3P sanma (`775`) from `riichienv-core` game
//!   states. Upstream `obs_shape` is `{1: 938, 2: 942, 3: 934, 4: 1012}`; see
//!   `tools/MORTAL_OBS_PARITY.md` for the full channel map.
//! - Mortal 46-action (4P) and 44-action (3P) legal action indexing and softmax ranking.

use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use candle_core::{DType, Device, Result as CandleResult, Tensor};
use candle_nn::VarBuilder;
use riichienv_core::action::{Action, ActionType};
use riichienv_core::state::GameState;
use riichienv_core::state_3p::GameState3P;

use crate::adapt::{enc_input_3p, enc_input_4p};
use crate::obs::EncInput;
use crate::tiles::{is_aka, tile34};

/// Mortal 4P action space size (`libriichi::consts::ACTION_SPACE`).
pub const MORTAL_ACTION_SPACE_4P: usize = 46;
/// Mortal 3P (`mortal-sanma`) action space size.
pub const MORTAL_ACTION_SPACE_3P: usize = 44;
/// Mortal tile axis width (both 4P and 3P `mortal-sanma` use 34 tile columns).
pub const MORTAL_TILE_DIM: usize = 34;

/// Metadata describing an active Mortal model (.pth or .onnx).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MortalMeta {
    pub num_players: u8,
    pub version: u8,
    pub in_channels: usize,
    pub conv_channels: usize,
    pub num_blocks: usize,
    pub action_space: usize,
}

/// Active custom model contract for a player count slot (4P or 3P).
#[derive(Clone)]
pub enum CustomContract {
    /// Standard Akagi `.onnx` contract (`[1, 39, 34] -> [1, 82]` or `[1, 37, 27] -> [1, 60]`).
    AkagiOnnx,
    /// Mortal `.onnx` evaluated via Android `OrtSession` callback.
    MortalOnnx(MortalMeta),
    /// Mortal `.pth` evaluated directly in Rust via Candle.
    MortalPth(Arc<MortalPthModel>),
}

static CONTRACT_4P: OnceLock<Mutex<CustomContract>> = OnceLock::new();
static CONTRACT_3P: OnceLock<Mutex<CustomContract>> = OnceLock::new();

fn contract_slot(num_players: u8) -> &'static Mutex<CustomContract> {
    if num_players == 3 {
        CONTRACT_3P.get_or_init(|| Mutex::new(CustomContract::AkagiOnnx))
    } else {
        CONTRACT_4P.get_or_init(|| Mutex::new(CustomContract::AkagiOnnx))
    }
}

/// Get the currently configured custom model contract for `num_players`.
pub fn active_contract(num_players: u8) -> CustomContract {
    contract_slot(num_players)
        .lock()
        .map(|g| g.clone())
        .unwrap_or(CustomContract::AkagiOnnx)
}

/// Configure a custom `.onnx` contract (`kind = 0` for Akagi, `kind = 1` for Mortal).
pub fn set_custom_onnx_contract(
    num_players: u8,
    kind: u8,
    version: u8,
    in_channels: usize,
    action_space: usize,
) {
    if let Ok(mut guard) = contract_slot(num_players).lock() {
        if kind == 1 && (num_players != 4 || version != 4 || in_channels != 1012 || action_space != 46) {
            return;
        }
        if kind == 1 {
            *guard = CustomContract::MortalOnnx(MortalMeta {
                num_players,
                version,
                in_channels,
                conv_channels: 0,
                num_blocks: 0,
                action_space,
            });
        } else {
            *guard = CustomContract::AkagiOnnx;
        }
    }
}

/// Load, validate, and activate a Mortal `.pth` checkpoint from `path`.
pub fn load_and_activate_pth(expected_players: u8, path: &str) -> i32 {
    load_pth(expected_players, path, true)
}
pub fn load_pth(expected_players: u8, path: &str, activate: bool) -> i32 {
    let model = match MortalPthModel::load_from_file(Path::new(path)) {
        Ok(m) => m,
        Err(_) => return -2,
    };
    let players = model.meta.num_players;
    if expected_players != 0 && expected_players != players {
        return -3;
    }
    let dummy_obs = vec![0.0f32; model.meta.in_channels * MORTAL_TILE_DIM];
    // A well-formed Mortal Q head returns values on the order of a game score
    // (roughly -10..+10). A head bound to the wrong tensor — for example a trunk
    // tensor picked up because the checkpoint's real head had an unexpected shape
    // — still produces `action_space` finite floats, so length and finiteness
    // alone cannot tell a good checkpoint from a broken binding. Bound the
    // magnitude as well, and report the observed range on failure.
    const SANE_Q_ABS_LIMIT: f32 = 1.0e4;
    match model.forward_logits(&dummy_obs) {
        Ok(logits)
            if logits.len() == model.meta.action_space
                && logits.iter().all(|v| v.is_finite())
                && logits.iter().all(|v| v.abs() <= SANE_Q_ABS_LIMIT) => {}
        Ok(logits) => {
            let lo = logits.iter().cloned().fold(f32::INFINITY, f32::min);
            let hi = logits.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            eprintln!(
                "mortal: refusing checkpoint — Q values are out of range \
(min={lo:.3}, max={hi:.3}, limit={SANE_Q_ABS_LIMIT}). The output head is \
probably bound to the wrong tensor."
            );
            return -6;
        }
        Err(e) => {
            eprintln!("mortal: forward pass failed: {e}");
            return -4;
        }
    }
    if !activate { return players as i32; }
    if let Ok(mut guard) = contract_slot(players).lock() {
        *guard = CustomContract::MortalPth(Arc::new(model));
        players as i32
    } else {
        -5
    }
}

#[derive(Clone)]
struct FoldedConv1d {
    weight: Tensor,
    bias: Tensor,
    padding: usize,
    /// `Some((actv, bn))` when a `[BatchNorm, actv]` pair preceded this
    /// convolution (Mortal's pre-activation blocks). `forward` then applies
    /// `bn -> actv -> conv` in that order.
    ///
    /// The BN affine CANNOT be folded into `weight`/`bias` here, because the
    /// activation sits *between* the BN and the conv: `conv(actv(bn(x)))` is not
    /// `conv_folded(actv(x))` for a nonlinear `actv`. Folding it would apply the
    /// activation to raw, un-normalised activations — which is exactly the kind
    /// of silent mismatch that yields plausible-looking but wrong Q values.
    pre_actv: Option<(Actv, (Tensor, Tensor))>,
}

impl FoldedConv1d {
    fn forward(&self, x: &Tensor) -> CandleResult<Tensor> {
        let x = match &self.pre_actv {
            Some((actv, (scale, shift))) => {
                // Upstream order is `[BN, actv, Conv]`.
                let n = x.broadcast_mul(scale)?.broadcast_add(shift)?;
                let a = actv.apply(&n)?;
                a
            }
            None => x.clone(),
        };
        let y = x.conv1d(&self.weight, self.padding, 1, 1, 1)?;
        y.broadcast_add(&self.bias)
    }

    /// A conv with no BN/activation prefix (used for the entry conv and the
    /// reduction conv in the tail).
    fn plain(weight: Tensor, bias: Tensor, padding: usize) -> Self {
        Self {
            weight,
            bias,
            padding,
            pre_actv: None,
        }
    }
}

/// BatchNorm `eps` used by `mortal/model.py`.
///
/// The model builder defaults to `eps=1e-5` and overrides it to `1e-3` for
/// versions 3 and 4. The crate does not track the exact version alongside the
/// activation, so this follows the same `>= 2` split the rest of the encoder
/// uses; v3 and v4 are the versions anyone actually runs.
fn norm_eps(actv: Actv) -> f64 {
    match actv {
        Actv::Relu => 1e-5,
        Actv::Mish => 1e-3,
    }
}

struct ChannelAttention {
    fc1_w: Tensor,
    fc1_b: Option<Tensor>,
    fc2_w: Tensor,
    fc2_b: Option<Tensor>,
}

impl ChannelAttention {
    /// `ChannelAttention.forward` from `mortal/model.py`:
    ///
    /// ```text
    /// avg_out = shared_mlp(x.mean(-1))
    /// max_out = shared_mlp(x.amax(-1))
    /// weight  = (avg_out + max_out).sigmoid()
    /// return weight.unsqueeze(-1) * x
    /// ```
    ///
    /// Both the average *and* the max over the tile axis feed the same MLP;
    /// using only the mean (as an earlier revision did) halves the signal.
    fn forward(&self, x: &Tensor) -> CandleResult<Tensor> {
        let avg = self.shared_mlp(&x.mean(2)?)?;
        let max = self.shared_mlp(&x.max(2)?)?;
        let weight = candle_nn::ops::sigmoid(&(avg + max)?)?.unsqueeze(2)?;
        x.broadcast_mul(&weight)
    }

    fn shared_mlp(&self, y: &Tensor) -> CandleResult<Tensor> {
        let mut y = y.matmul(&self.fc1_w.t()?)?;
        if let Some(b) = &self.fc1_b {
            y = y.broadcast_add(b)?;
        }
        // The builder's activation for v2..=v4 is Mish.
        let y = mish(&y)?;
        let mut y = y.matmul(&self.fc2_w.t()?)?;
        if let Some(b) = &self.fc2_b {
            y = y.broadcast_add(b)?;
        }
        Ok(y)
    }
}

/// `nn.Mish`: `x * tanh(softplus(x))`.
///
/// candle has `silu` built in but not Mish, and Mortal uses Mish for every
/// v2..=v4 activation (`Brain`/`ResNet`/`ResBlock`/`ChannelAttention`), so it
/// has to be exact rather than approximated by SiLU.
///
/// `softplus(x) = ln(1 + e^x)` is computed in the numerically stable form
/// `max(x, 0) + ln(1 + e^-|x|)`, which is what PyTorch's `softplus` does with
/// the default `beta = 1` and `threshold = 20`.
fn mish(x: &Tensor) -> CandleResult<Tensor> {
    let sp = softplus(x)?;
    x.broadcast_mul(&sp.tanh()?)
}

fn softplus(x: &Tensor) -> CandleResult<Tensor> {
    // For large |x| the log1p term vanishes, matching PyTorch's thresholding.
    let abs = x.abs()?;
    let neg_abs = abs.neg()?;
    let one = Tensor::ones_like(&neg_abs)?;
    let log1p = (neg_abs.exp()? + one)?.log()?;
    let relu = x.relu()?;
    relu + log1p
}

/// Mortal's activation for a given version.
///
/// v1 uses ReLU and a post-activation ResBlock; v2..=v4 use Mish with a
/// pre-activation ResBlock.
fn is_pre_actv(version: u8) -> bool {
    version >= 2
}

/// The activation function selected by `actv_builder` in `mortal/model.py`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actv {
    /// v1.
    Relu,
    /// v2..=v4.
    Mish,
}

impl Actv {
    pub fn for_version(version: u8) -> Self {
        if version >= 2 {
            Actv::Mish
        } else {
            Actv::Relu
        }
    }

    pub fn apply(self, x: &Tensor) -> CandleResult<Tensor> {
        match self {
            Actv::Relu => x.relu(),
            Actv::Mish => mish(x),
        }
    }
}

struct ResBlock {
    /// Carries the `BN -> actv` prefix folded in (see `FoldedConv1d`).
    conv1: FoldedConv1d,
    /// Carries the `BN -> actv` prefix folded in, or a plain conv when the
    /// checkpoint uses the v1 post-activation layout.
    conv2: FoldedConv1d,
    ca: Option<ChannelAttention>,
}

impl ResBlock {
    /// `ResBlock.forward` from `mortal/model.py`:
    ///
    /// ```text
    /// out = self.res_unit(x)   # [BN, actv, Conv, BN, actv, Conv] when pre_actv
    /// out = self.ca(out)
    /// out = out + x
    /// if not pre_actv: out = self.actv(out)
    /// ```
    ///
    /// With `pre_actv` there is **no** trailing activation: the block output is
    /// the plain residual sum, and the following block's leading BatchNorm
    /// handles normalisation.
    fn forward(&self, x: &Tensor) -> CandleResult<Tensor> {
        let mut out = self.conv1.forward(x)?;
        out = self.conv2.forward(&out)?;
        if let Some(ca) = &self.ca {
            out = ca.forward(&out)?;
        }
        x + out
    }
}

enum OutputHead {
    DuelingCombined {
        weight: Tensor,
        bias: Tensor,
        action_space: usize,
        feat_dim: usize,
    },
    DuelingSplit {
        v_w: Tensor,
        v_b: Tensor,
        a_w: Tensor,
        a_b: Tensor,
        feat_dim: usize,
    },
    Direct {
        weight: Tensor,
        bias: Tensor,
        feat_dim: usize,
    },
}

/// Mortal's post-trunk tail: `net.55` BatchNorm1d, `net.57` Conv1d, `net.60`
/// Linear. The convolution reduces the 256-channel trunk output to 32 channels,
/// which are flattened (32 x 34) into a 1088-wide vector before the final
/// Linear projection.
#[derive(Clone)]
struct PostConvStage {
    bn_scale: Option<(Tensor, Tensor)>,
    conv: Option<FoldedConv1d>,
    /// Final Linear: (weight, bias) applied to the flattened vector.
    fc: Option<(Tensor, Tensor)>,
}

impl PostConvStage {
    fn is_empty(&self) -> bool {
        self.bn_scale.is_none() && self.conv.is_none() && self.fc.is_none()
    }

    fn apply(&self, h: &Tensor, actv: Actv) -> CandleResult<Tensor> {
        let mut cur = h.clone();
        if let Some((scale, shift)) = &self.bn_scale {
            cur = cur.broadcast_mul(scale)?.broadcast_add(shift)?;
            cur = actv.apply(&cur)?;
        }
        if let Some(conv) = &self.conv {
            cur = conv.forward(&cur)?;
            cur = actv.apply(&cur)?;
        }
        if let Some((w, b)) = &self.fc {
            cur = cur.flatten_from(1)?.matmul(&w.t()?)?.broadcast_add(b)?;
            cur = actv.apply(&cur)?;
        }
        Ok(cur)
    }
}

pub struct MortalPthModel {
    pub meta: MortalMeta,
    entry_conv: FoldedConv1d,
    blocks: Vec<ResBlock>,
    post_bn_scale: Option<(Tensor, Tensor)>,
    post_conv: PostConvStage,
    head_fc1: Option<(Tensor, Tensor)>,
    head: OutputHead,
    actv: Actv,
}

impl MortalPthModel {
    pub fn load_from_file(path: &Path) -> CandleResult<Self> {
        let dev = Device::Cpu;
        let brain_vb = Self::open_brain_vb(path, &dev)?;
        let dqn_vb = Self::open_dqn_vb(path, &dev)?;
        let (entry_conv, conv_channels, in_channels) = Self::load_entry_conv(&brain_vb, &dev)?;
        if conv_channels > 512 { return Err(candle_core::Error::Msg("Mortal trunk exceeds supported width".into())); }

        // `in_channels` is the entry convolution's depth, which is exactly
        // libriichi's `obs_shape(version).0`. Upstream Mortal only defines
        // versions 1..=4 (4-player); 775 comes from the separate sanma fork
        // (`libriichi3p`), whose task space is 44.
        //
        // Do not confuse v2/v3: upstream is `{1: 938, 2: 942, 3: 934, 4: 1012}`,
        // so v3 (934) is *narrower* than v2 (942) and v1 (938).
        if in_channels != 1012 {
            return Err(candle_core::Error::Msg("Only experimental four-player Mortal v4 (1012 channels) is supported".into()));
        }
        let (num_players, version, action_space) = (4u8, 4u8, MORTAL_ACTION_SPACE_4P);

        let actv = Actv::for_version(version);
        let mut blocks = Vec::new();
        for idx in 0..128usize {
            if let Some(block) = Self::try_load_res_block(&brain_vb, idx, conv_channels, actv, &dev)? {
                blocks.push(block);
            } else if idx > 0 {
                break;
            }
        }
        if blocks.is_empty() {
            return Err(candle_core::Error::Msg(
                "No ResNet blocks found in Mortal .pth checkpoint".into(),
            ));
        }
        let num_blocks = blocks.len();
        let post_bn_scale = Self::try_load_standalone_bn(&brain_vb, conv_channels)?;
        let post_conv = Self::load_post_conv_stage(&brain_vb, conv_channels, &dev)?;
        if post_conv.bn_scale.is_none() || post_conv.conv.is_none() || post_conv.fc.is_none() {
            return Err(candle_core::Error::Msg("Incomplete Mortal v4 tail".into()));
        }
        let (head_fc1, head) = Self::load_output_head(&dqn_vb, &brain_vb, conv_channels, action_space)?;

        Ok(Self {
            meta: MortalMeta {
                num_players,
                version,
                in_channels,
                conv_channels,
                num_blocks,
                action_space,
            },
            entry_conv,
            blocks,
            post_bn_scale,
            post_conv,
            head_fc1,
            head,
            actv,
        })
    }

    pub fn forward_logits(&self, obs: &[f32]) -> CandleResult<Vec<f32>> {
        let expected = self.meta.in_channels * MORTAL_TILE_DIM;
        if obs.len() != expected {
            return Err(candle_core::Error::Msg(format!(
                "Mortal obs length mismatch: expected {expected}, got {}",
                obs.len()
            )));
        }
        let dev = Device::Cpu;
        let x = Tensor::from_slice(obs, (1, self.meta.in_channels, MORTAL_TILE_DIM), &dev)?;

        let mut h = self.entry_conv.forward(&x)?;
        // v4 starts with a plain convolution; the first block owns BN + Mish.

        for block in &self.blocks {
            h = block.forward(&h)?;
        }

        // Mortal's tail, in upstream `ResNet.net` order:
        //   ... [BN, actv] [Conv1d(256->32), actv] [Flatten] [Linear(1088->1024)]
        // `PostConvStage::apply` walks that whole sequence, including the final
        // BatchNorm1d. `try_load_standalone_bn` resolves the *same* `net.55`
        // tensor, so applying it here as well would run the BN (and its
        // activation) twice, exponentially inflating the trunk output. Only use
        // it when the stage did not already consume it.
        if !self.post_conv.is_empty() {
            h = self.post_conv.apply(&h, self.actv)?;
        } else if let Some((scale, bias)) = &self.post_bn_scale {
            h = h.broadcast_mul(scale)?.broadcast_add(bias)?;
            h = self.actv.apply(&h)?;
        }

        // When the Mortal tail already flattened and projected the feature
        // vector (net.60), `h` is (1, feat_dim) and must not be pooled again.
        let mut feat = if self.post_conv.fc.is_some() {
            h
        } else {
            let target_feat_dim = match &self.head_fc1 {
                Some((w, _)) => w.dims()[1],
                None => match &self.head {
                    OutputHead::DuelingCombined { feat_dim, .. }
                    | OutputHead::DuelingSplit { feat_dim, .. }
                    | OutputHead::Direct { feat_dim, .. } => *feat_dim,
                },
            };
            if target_feat_dim == self.meta.conv_channels * MORTAL_TILE_DIM {
                h.flatten_from(1)?
            } else {
                h.mean(2)?
            }
        };

        if let Some((fc1_w, fc1_b)) = &self.head_fc1 {
            feat = feat.matmul(&fc1_w.t()?)?.broadcast_add(fc1_b)?;
            feat = self.actv.apply(&feat)?;
        }

        let q = match &self.head {
            OutputHead::DuelingCombined {
                weight,
                bias,
                action_space,
                ..
            } => {
                let va = feat.matmul(&weight.t()?)?.broadcast_add(bias)?;
                let v = va.narrow(1, 0, 1)?;
                let a = va.narrow(1, 1, *action_space)?;
                let a_mean = a.mean_keepdim(1)?;
                v.broadcast_add(&a.broadcast_sub(&a_mean)?)?
            }
            OutputHead::DuelingSplit {
                v_w,
                v_b,
                a_w,
                a_b,
                ..
            } => {
                let v = feat.matmul(&v_w.t()?)?.broadcast_add(v_b)?;
                let a = feat.matmul(&a_w.t()?)?.broadcast_add(a_b)?;
                let a_mean = a.mean_keepdim(1)?;
                v.broadcast_add(&a.broadcast_sub(&a_mean)?)?
            }
            OutputHead::Direct { weight, bias, .. } => {
                feat.matmul(&weight.t()?)?.broadcast_add(bias)?
            }
        };

        q.squeeze(0)?.to_vec1::<f32>()
    }

    fn open_brain_vb<'a>(path: &Path, dev: &Device) -> CandleResult<VarBuilder<'a>> {
        // Mortal checkpoints nest the trunk under `mortal.encoder.net.*`, while
        // Akagi exports use flatter trunks. Try every known root, then try the
        // resolved root's `encoder.net` / `encoder` / `net` child as well.
        for key in ["mortal", "brain", "model", "state_dict"] {
            if let Ok(vb) = VarBuilder::from_pth_with_state(path, DType::F32, key, dev) {
                if let Some(sub) = Self::descend_to_trunk(&vb) {
                    return Ok(sub);
                }
            }
        }
        let flat = VarBuilder::from_pth(path, DType::F32, dev)?;
        for prefix in ["mortal", "brain", "model"] {
            let sub = flat.pp(prefix);
            if let Some(found) = Self::descend_to_trunk(&sub) {
                return Ok(found);
            }
        }
        Ok(flat)
    }

    /// Given a VarBuilder pointing at a candidate state_dict root, walk down the
    /// known trunk paths until the entry convolution is reachable.
    fn descend_to_trunk<'a>(vb: &VarBuilder<'a>) -> Option<VarBuilder<'a>> {
        if Self::has_any_entry_conv(vb) {
            return Some(vb.clone());
        }
        for child in ["encoder.net", "net", "encoder", "backbone"] {
            let sub = vb.pp(child);
            if Self::has_any_entry_conv(&sub) {
                return Some(sub);
            }
        }
        None
    }

    fn open_dqn_vb<'a>(path: &Path, dev: &Device) -> CandleResult<VarBuilder<'a>> {
        for key in ["current_dqn", "dqn", "target_dqn", "head", "policy"] {
            if let Ok(vb) = VarBuilder::from_pth_with_state(path, DType::F32, key, dev) {
                if Self::has_any_head_tensor(&vb) {
                    return Ok(vb);
                }
            }
        }
        let flat = VarBuilder::from_pth(path, DType::F32, dev)?;
        for prefix in ["current_dqn", "dqn", "head", "mortal"] {
            let sub = flat.pp(prefix);
            if Self::has_any_head_tensor(&sub) {
                return Ok(sub);
            }
        }
        Ok(flat)
    }
    fn entry_conv_candidates() -> &'static [&'static str] {
        &[
            // `open_brain_vb` normally descends into the trunk, so the bare
            // `0` (entry conv) form is what resolves for real Mortal
            // checkpoints (`mortal.encoder.net.0`). The qualified forms cover
            // the non-descended case.
            "0",
            "encoder.net.0",
            "net.0",
            // Akagi's own bundled / exported variants.
            "entry.0",
            "entry",
            "stem.0",
            "stem",
            "conv_in.0",
            "conv_in",
            "in_conv.0",
            "in_conv",
            "backbone.entry.0",
            "backbone.0",
        ]
    }

    fn has_any_entry_conv(vb: &VarBuilder) -> bool {
        Self::entry_conv_candidates()
            .iter()
            .any(|p| vb.contains_tensor(&format!("{p}.weight")))
    }

    fn has_any_head_tensor(vb: &VarBuilder) -> bool {
        [
            // Real Mortal: the head lives at `current_dqn.net`.
            "net.weight",
            "net.bias",
            "v_a.weight",
            "v_a.0.weight",
            "v_a.2.weight",
            "fc.weight",
            "head.weight",
            "out.weight",
            "a_head.weight",
            "0.weight",
            "2.weight",
        ]
        .iter()
        .any(|k| vb.contains_tensor(k))
    }

    fn load_entry_conv(
        vb: &VarBuilder,
        dev: &Device,
    ) -> CandleResult<(FoldedConv1d, usize, usize)> {
        for &prefix in Self::entry_conv_candidates() {
            let w_name = format!("{prefix}.weight");
            if vb.contains_tensor(&w_name) {
                let w = vb.get_unchecked_dtype(&w_name, DType::F32)?;
                if w.dims().len() == 3 {
                    let conv_channels = w.dims()[0];
                    let in_channels = w.dims()[1];
                    let bn_candidates = if let Some(stem) = prefix.strip_suffix(".0") {
                        vec![format!("{stem}.1"), format!("{stem}.bn"), "bn1".to_string()]
                    } else {
                        vec![
                            format!("{prefix}_bn"),
                            "entry.1".to_string(),
                            "bn1".to_string(),
                            "1".to_string(),
                        ]
                    };
                    let folded = Self::fold_conv_with_bn(vb, prefix, &bn_candidates, conv_channels, dev, Actv::for_version(1))?;
                    return Ok((folded, conv_channels, in_channels));
                }
            }
        }
        Err(candle_core::Error::Msg(
            "Unable to find entry Conv1d weight in Mortal .pth".into(),
        ))
    }

    /// Fold a BatchNorm into a convolution, and remember the activation that
    /// separates them.
    ///
    /// Mortal's pre-activation blocks are `[BN, actv, Conv, ...]`, so the
    /// activation is *nonlinear* and sits between the BN and the conv — it
    /// cannot be folded into the weights. The returned `FoldedConv1d` therefore
    /// carries the BN affine as a folded bias and records `pre_actv`, and
    /// `forward` applies `actv -> conv` in that order.
    fn fold_conv_with_bn(
        vb: &VarBuilder,
        conv_prefix: &str,
        bn_prefixes: &[String],
        out_channels: usize,
        dev: &Device,
        actv: Actv,
    ) -> CandleResult<FoldedConv1d> {
        let w = vb.get_unchecked_dtype(&format!("{conv_prefix}.weight"), DType::F32)?;
        let ksize = w.dims()[2];
        let padding = if ksize == 3 { 1 } else { 0 };
        let b_name = format!("{conv_prefix}.bias");
        let conv_b = if vb.contains_tensor(&b_name) {
            vb.get_unchecked_dtype(&b_name, DType::F32)?
        } else {
            Tensor::zeros((out_channels,), DType::F32, dev)?
        };

        for bn_p in bn_prefixes {
            let mean_k = format!("{bn_p}.running_mean");
            let var_k = format!("{bn_p}.running_var");
            if vb.contains_tensor(&mean_k) && vb.contains_tensor(&var_k) {
                let mean = vb.get_unchecked_dtype(&mean_k, DType::F32)?;
                let var = vb.get_unchecked_dtype(&var_k, DType::F32)?;
                let gamma_k = format!("{bn_p}.weight");
                let beta_k = format!("{bn_p}.bias");
                let gamma = if vb.contains_tensor(&gamma_k) {
                    vb.get_unchecked_dtype(&gamma_k, DType::F32)?
                } else {
                    Tensor::ones((out_channels,), DType::F32, dev)?
                };
                let beta = if vb.contains_tensor(&beta_k) {
                    vb.get_unchecked_dtype(&beta_k, DType::F32)?
                } else {
                    Tensor::zeros((out_channels,), DType::F32, dev)?
                };

                // The BN affine stays separate: the activation sits between the
                // BN and the convolution, so folding BN into the weights would
                // change the function (see `FoldedConv1d::pre_actv`).
                let std = (var + norm_eps(actv))?.sqrt()?;
                let scale = (&gamma / &std)?.reshape((1, out_channels, 1))?;
                let shift =
                    (&beta - (&mean * (&gamma / &std)?)?)?.reshape((1, out_channels, 1))?;
                return Ok(FoldedConv1d {
                    weight: w,
                    bias: conv_b.reshape((1, out_channels, 1))?,
                    padding,
                    pre_actv: Some((actv, (scale, shift))),
                });
            }
        }

        Ok(FoldedConv1d {
            weight: w,
            bias: conv_b.reshape((1, out_channels, 1))?,
            padding,
            pre_actv: None,
        })
    }

    fn try_load_res_block(
        vb: &VarBuilder,
        idx: usize,
        channels: usize,
        actv: Actv,
        dev: &Device,
    ) -> CandleResult<Option<ResBlock>> {
        // `idx` is the res-block ordinal (0-based). Real Mortal checkpoints
        // place block N at `encoder.net.{N+1}` (the entry conv occupies
        // `net.0`). `open_brain_vb` normally already descended into the trunk,
        // so the bare `{N}.res_unit` form is what resolves there; the
        // fully-qualified forms cover the non-descended case.
        //
        // NOTE: there must be no `encoder.net.{idx}.res_unit` candidate here.
        // That form is off by one for real checkpoints, so it never matches a
        // new block - but it *does* match the previous block again, which
        // appends a duplicate ResBlock and silently changes the forward pass.
        let block_prefixes = [
            format!("{}.res_unit", idx + 1),
            format!("encoder.net.{}.res_unit", idx + 1),
            format!("net.{}.res_unit", idx + 1),
            // Akagi exports.
            format!("backbone.blocks.{idx}"),
            format!("blocks.{idx}"),
            format!("backbone.{idx}"),
            format!("layers.{idx}"),
            format!("res.{idx}"),
        ];

        for bp in &block_prefixes {
            let sub = vb.pp(bp);
            // Layouts are (conv1, bn1, conv2, bn2).
            //
            // Real Mortal (`pre_actv = true`, v2..=v4) lays `res_unit` out as
            //   [BN, actv, Conv, BN, actv, Conv]
            // so the BatchNorm at index 0 immediately precedes the conv at
            // index 2, which is what makes folding sound.
            //
            // The v1 (`pre_actv = false`) layout is
            //   [Conv, BN, actv, Conv, BN]
            // and the Akagi exports use the named forms.
            let pre_actv = is_pre_actv(match actv {
                Actv::Relu => 1,
                Actv::Mish => 2,
            });
            let conv_layouts: Vec<(&str, Vec<String>, &str, Vec<String>)> = if pre_actv {
                vec![
                    ("2", vec!["0".to_string()], "5", vec!["3".to_string()]),
                    ("conv1.0", vec!["conv1.1".to_string()], "conv2.0", vec!["conv2.1".to_string()]),
                    ("conv1", vec!["bn1".to_string(), "conv1_bn".to_string()], "conv2", vec!["bn2".to_string(), "conv2_bn".to_string()]),
                    ("0", vec!["1".to_string()], "3", vec!["4".to_string()]),
                    ("block.0", vec!["block.1".to_string()], "block.3", vec!["block.4".to_string()]),
                ]
            } else {
                vec![
                    ("conv1.0", vec!["conv1.1".to_string()], "conv2.0", vec!["conv2.1".to_string()]),
                    ("conv1", vec!["bn1".to_string(), "conv1_bn".to_string()], "conv2", vec!["bn2".to_string(), "conv2_bn".to_string()]),
                    ("2", vec!["0".to_string()], "5", vec!["3".to_string()]),
                    ("0", vec!["1".to_string()], "3", vec!["4".to_string()]),
                    ("block.0", vec!["block.1".to_string()], "block.3", vec!["block.4".to_string()]),
                ]
            };

            for (c1_p, bn1_p, c2_p, bn2_p) in conv_layouts {
                if sub.contains_tensor(&format!("{c1_p}.weight"))
                    && sub.contains_tensor(&format!("{c2_p}.weight"))
                {
                    // With `pre_actv`, the convolution consumes the *activated*
                    // BatchNorm output, so activation must be baked into the
                    // folded conv. The fold below applies BN only, hence the
                    // explicit pre-activation step in `ResBlock::forward` is
                    // left to the caller via `actv`; here we only need the
                    // affine BN parameters.
                    let conv1 = Self::fold_conv_with_bn(&sub, c1_p, &bn1_p, channels, dev, actv)?;
                    let conv2 = Self::fold_conv_with_bn(&sub, c2_p, &bn2_p, channels, dev, actv)?;
                    // In real Mortal checkpoints the channel attention is a
                    // SIBLING of `res_unit` (e.g. `encoder.net.N.ca...`), not a
                    // child, so it is only reachable from the *parent* scope.
                    // `sub` points at `<bp>` itself, so the second probe has to
                    // strip the trailing `.res_unit` segment rather than re-apply
                    // the same prefix (which would search the same place twice
                    // and always miss).
                    let parent = bp.rsplit_once('.').map(|(head, _)| head.to_string());
                    let ca = match Self::try_load_ca(&sub)? {
                        Some(ca) => Some(ca),
                        None => match parent {
                            Some(p) => Self::try_load_ca(&vb.pp(p))?,
                            None => None,
                        },
                    };
                    if ca.is_none() { return Err(candle_core::Error::Msg("Missing Mortal v4 channel attention".into())); }
                    return Ok(Some(ResBlock { conv1, conv2, ca }));
                }
            }
        }
        Ok(None)
    }

    fn try_load_ca(block_vb: &VarBuilder) -> CandleResult<Option<ChannelAttention>> {
        let ca_layouts = [
            // Real Mortal: `ca.shared_mlp.0` -> `ca.shared_mlp.2`,
            // where `shared_mlp.1` is the activation in between.
            ("ca.shared_mlp.0", "ca.shared_mlp.2"),
            ("ca.fc1", "ca.fc2"),
            ("ca.0", "ca.2"),
            ("se.fc1", "se.fc2"),
            ("se.0", "se.2"),
            ("attn.0", "attn.2"),
            ("channel_attention.0", "channel_attention.2"),
        ];
        for (p1, p2) in ca_layouts {
            let w1_k = format!("{p1}.weight");
            let w2_k = format!("{p2}.weight");
            if block_vb.contains_tensor(&w1_k) && block_vb.contains_tensor(&w2_k) {
                let mut fc1_w = block_vb.get_unchecked_dtype(&w1_k, DType::F32)?;
                let mut fc2_w = block_vb.get_unchecked_dtype(&w2_k, DType::F32)?;
                if fc1_w.dims().len() == 3 {
                    fc1_w = fc1_w.squeeze(2)?;
                }
                if fc2_w.dims().len() == 3 {
                    fc2_w = fc2_w.squeeze(2)?;
                }
                let b1_k = format!("{p1}.bias");
                let b2_k = format!("{p2}.bias");
                let fc1_b = if block_vb.contains_tensor(&b1_k) {
                    Some(block_vb.get_unchecked_dtype(&b1_k, DType::F32)?.reshape((1, fc1_w.dims()[0]))?)
                } else {
                    None
                };
                let fc2_b = if block_vb.contains_tensor(&b2_k) {
                    Some(block_vb.get_unchecked_dtype(&b2_k, DType::F32)?.reshape((1, fc2_w.dims()[0]))?)
                } else {
                    None
                };
                return Ok(Some(ChannelAttention {
                    fc1_w,
                    fc1_b,
                    fc2_w,
                    fc2_b,
                }));
            }
        }
        Ok(None)
    }

    /// Load Mortal's `net.55` BatchNorm1d, `net.57` Conv1d and `net.60` Linear.
    ///
    /// In the reference Mortal checkpoints the trunk ends at `net.<2N+1>` for N
    /// res blocks; the tail modules then follow at fixed offsets. We locate them
    /// structurally rather than by hard-coded index so that other depths work.
    fn load_post_conv_stage(
        vb: &VarBuilder,
        conv_channels: usize,
        dev: &Device,
    ) -> CandleResult<PostConvStage> {
        // `vb` normally points at the trunk root (`encoder.net`). Res blocks
        // are the indices that expose `res_unit`; the entry conv and the tail
        // modules are the ones that expose a bare `.weight`.
        let has_res_unit = |i: usize| -> bool {
            vb.contains_tensor(&format!("{i}.res_unit.2.weight"))
                || vb.contains_tensor(&format!("net.{i}.res_unit.2.weight"))
                || vb.contains_tensor(&format!("encoder.net.{i}.res_unit.2.weight"))
        };
        let has_module = |i: usize| -> bool {
            vb.contains_tensor(&format!("{i}.weight"))
                || vb.contains_tensor(&format!("net.{i}.weight"))
                || vb.contains_tensor(&format!("encoder.net.{i}.weight"))
                || vb.contains_tensor(&format!("{i}.running_mean"))
                || vb.contains_tensor(&format!("net.{i}.running_mean"))
                || vb.contains_tensor(&format!("encoder.net.{i}.running_mean"))
        };

        // Last contiguous res-block index, i.e. the number of blocks.
        let mut last_block = 0usize;
        let mut i = 1usize;
        while i < 256 && has_res_unit(i) {
            last_block = i;
            i += 1;
        }

        // Tail module indices come after the block run.
        let mut tail: Vec<usize> = Vec::new();
        for idx in (last_block + 1)..=255usize {
            if has_module(idx) {
                tail.push(idx);
            }
        }

        let mut stage = PostConvStage {
            bn_scale: None,
            conv: None,
            fc: None,
        };

        // Resolve a tail module index to the key prefix that actually exists,
        // trying the bare form first (the trunk is usually already descended).
        let resolve = |i: usize| -> Option<String> {
            for p in [
                format!("net.{i}"),
                format!("{i}"),
                format!("encoder.net.{i}"),
            ] {
                if vb.contains_tensor(&format!("{p}.weight")) {
                    return Some(p);
                }
            }
            None
        };

        // BatchNorm1d (1-D weight, has running_mean, no 3-D conv shape).
        for &i in &tail {
            if let Some(p) = resolve(i) {
                if vb.contains_tensor(&format!("{p}.running_mean")) {
                    let w = vb.get_unchecked_dtype(&format!("{p}.weight"), DType::F32)?;
                    if w.dims().len() == 1 && w.dims()[0] == conv_channels {
                        let mean = vb.get_unchecked_dtype(&format!("{p}.running_mean"), DType::F32)?;
                        let var = vb.get_unchecked_dtype(&format!("{p}.running_var"), DType::F32)?;
                        let beta = vb.get_unchecked_dtype(&format!("{p}.bias"), DType::F32)?;
                        let std = (var + 1e-3f64)?.sqrt()?;
                        let scale = (&w / &std)?.reshape((1, conv_channels, 1))?;
                        let shift =
                            (&beta - (&mean * (&w / &std)?)?)?.reshape((1, conv_channels, 1))?;
                        stage.bn_scale = Some((scale, shift));
                        break;
                    }
                }
            }
        }

        // Conv1d with out_channels != conv_channels (channel reduction).
        for &i in &tail {
            if let Some(p) = resolve(i) {
                if let Ok(w) = vb.get_unchecked_dtype(&format!("{p}.weight"), DType::F32) {
                    if w.dims().len() == 3 && w.dims()[0] != conv_channels {
                        let out_ch = w.dims()[0];
                        let bias = if vb.contains_tensor(&format!("{p}.bias")) {
                            vb.get_unchecked_dtype(&format!("{p}.bias"), DType::F32)?
                        } else {
                            Tensor::zeros((out_ch,), DType::F32, dev)?
                        };
                        let ksize = w.dims()[2];
                        let padding = if ksize == 3 { 1 } else { 0 };
                        stage.conv = Some(FoldedConv1d::plain(
                            w,
                            bias.reshape((1, out_ch, 1))?,
                            padding,
                        ));
                        break;
                    }
                }
            }
        }

        // Final Linear projection: its input width is `out_ch * TILE_DIM` where
        // `out_ch` is the channel count produced by the reduction conv above
        // (32 in the reference v4 model -> 32 * 34 = 1088).
        let flat_width = stage
            .conv
            .as_ref()
            .map(|c| c.weight.dims()[0] * MORTAL_TILE_DIM);
        for &i in &tail {
            if let Some(p) = resolve(i) {
                if let Ok(w) = vb.get_unchecked_dtype(&format!("{p}.weight"), DType::F32) {
                    if w.dims().len() == 2
                        && flat_width.is_some_and(|fw| w.dims()[1] == fw)
                    {
                        let b = vb
                            .get_unchecked_dtype(&format!("{p}.bias"), DType::F32)?
                            .reshape((1, w.dims()[0]))?;
                        stage.fc = Some((w, b));
                        break;
                    }
                }
            }
        }

        Ok(stage)
    }

    fn try_load_standalone_bn(
        vb: &VarBuilder,
        channels: usize,
    ) -> CandleResult<Option<(Tensor, Tensor)>> {
        // NOTE: real Mortal checkpoints keep their final BatchNorm1d in the
        // numbered trunk (`net.<2N+1>`, e.g. `net.55`), which
        // `load_post_conv_stage` already consumes. Only the named variants of
        // other exports are matched here, so the same BN is never applied
        // twice.
        for prefix in ["backbone.bn", "bn_out", "out_bn", "norm", "final_bn"] {
            let mean_k = format!("{prefix}.running_mean");
            let var_k = format!("{prefix}.running_var");
            if vb.contains_tensor(&mean_k) && vb.contains_tensor(&var_k) {
                let mean = vb.get_unchecked_dtype(&mean_k, DType::F32)?;
                let var = vb.get_unchecked_dtype(&var_k, DType::F32)?;
                let gamma = vb.get_unchecked_dtype(&format!("{prefix}.weight"), DType::F32)?;
                let beta = vb.get_unchecked_dtype(&format!("{prefix}.bias"), DType::F32)?;
                let std = (var + 1e-3f64)?.sqrt()?;
                let scale = (&gamma / &std)?.reshape((1, channels, 1))?;
                let shift = (&beta - (&mean * (&gamma / &std)?)?)?.reshape((1, channels, 1))?;
                return Ok(Some((scale, shift)));
            }
        }
        Ok(None)
    }

    fn load_output_head(
        dqn_vb: &VarBuilder,
        brain_vb: &VarBuilder,
        conv_channels: usize,
        action_space: usize,
    ) -> CandleResult<(Option<(Tensor, Tensor)>, OutputHead)> {
        // The output head MUST come from the DQN scope. Searching `brain_vb`
        // (the trunk) as a fallback is how a checkpoint whose head does not match
        // the expected shape ends up silently bound to an unrelated 2-D trunk
        // tensor: the loader reports success, and inference returns garbage.
        //
        // A real Mortal DQN head is one of:
        //   v4+      `net`         -> [1 + ACTION_SPACE, F]
        //   v2/v3    `v_head`/`a_head` (or `v_a`) -> [1, F] and [ACTION_SPACE, F]
        //   exported `head`/`policy`/... -> [ACTION_SPACE, F]
        //
        // Anything else is a hard error that names what was found, so the message
        // is actionable instead of a silently bad game.
        let _ = brain_vb;
        let mut head_fc1 = None;
        for fc1_p in ["v_a.0", "fc1", "fc", "net.0", "0"] {
            let wk = format!("{fc1_p}.weight");
            let bk = format!("{fc1_p}.bias");
            if dqn_vb.contains_tensor(&wk) && dqn_vb.contains_tensor(&bk) {
                if let Ok(w) = dqn_vb.get_unchecked_dtype(&wk, DType::F32) {
                    if w.dims().len() == 2
                        && w.dims()[0] != action_space + 1
                        && w.dims()[0] != action_space
                        && w.dims()[0] != 1
                    {
                        let b = dqn_vb
                            .get_unchecked_dtype(&bk, DType::F32)?
                            .reshape((1, w.dims()[0]))?;
                        head_fc1 = Some((w, b));
                        break;
                    }
                }
            }
        }

        // --- 1. Dueling, combined: one [1 + ACTION_SPACE, F] matrix -------------
        for p in ["v_a", "v_a.2", "v_a.1", "net", "head", "out", "fc_out", "2", "1", "0"] {
            let wk = format!("{p}.weight");
            let bk = format!("{p}.bias");
            if !dqn_vb.contains_tensor(&wk) || !dqn_vb.contains_tensor(&bk) {
                continue;
            }
            let w = dqn_vb.get_unchecked_dtype(&wk, DType::F32)?;
            if w.dims().len() != 2 {
                continue;
            }
            if w.dims()[0] == action_space + 1 {
                let feat_dim = w.dims()[1];
                let b = dqn_vb
                    .get_unchecked_dtype(&bk, DType::F32)?
                    .reshape((1, action_space + 1))?;
                return Ok((
                    head_fc1,
                    OutputHead::DuelingCombined {
                        weight: w,
                        bias: b,
                        action_space,
                        feat_dim,
                    },
                ));
            }
            // A 2-D head in the DQN scope with the wrong row count is the exact
            // signature of a mismatched checkpoint. Refuse it loudly.
            let found = w.dims()[0];
            let expected = action_space + 1;
            let hint = if found % action_space == 0 {
                format!(
                    " ({} = {} x {}, which looks like a sequence/multi-head output \
rather than a single-step DQN head)",
                    found,
                    action_space,
                    found / action_space
                )
            } else {
                String::new()
            };
            return Err(candle_core::Error::Msg(format!(
                "Mortal DQN head has the wrong shape: '{wk}' is [{found}, {}], \
expected [{expected}, F] for a {action_space}-action dueling head{hint}. \
This checkpoint is not a plain Mortal DQN export.",
                w.dims()[1]
            )));
        }

        // --- 2. Dueling, split value/advantage heads ---------------------------
        for (vp, ap) in [
            ("v_head", "a_head"),
            ("v", "a"),
            ("value", "advantage"),
            ("val", "adv"),
        ] {
            let (vw, ab) = (format!("{vp}.weight"), format!("{ap}.weight"));
            if !dqn_vb.contains_tensor(&vw) || !dqn_vb.contains_tensor(&ab) {
                continue;
            }
            let v_w = dqn_vb.get_unchecked_dtype(&vw, DType::F32)?;
            let v_b = dqn_vb
                .get_unchecked_dtype(&format!("{vp}.bias"), DType::F32)?
                .reshape((1, 1))?;
            let a_w = dqn_vb.get_unchecked_dtype(&ab, DType::F32)?;
            if a_w.dims().len() != 2 || a_w.dims()[0] != action_space {
                return Err(candle_core::Error::Msg(format!(
                    "Mortal advantage head '{ab}' is {:?}, expected [{}, F]",
                    a_w.dims(),
                    action_space
                )));
            }
            let a_b = dqn_vb
                .get_unchecked_dtype(&format!("{ap}.bias"), DType::F32)?
                .reshape((1, action_space))?;
            let feat_dim = a_w.dims()[1];
            return Ok((
                head_fc1,
                OutputHead::DuelingSplit {
                    v_w,
                    v_b,
                    a_w,
                    a_b,
                    feat_dim,
                },
            ));
        }

        // --- 3. Plain policy head ---------------------------------------------
        for p in ["head", "policy", "actor", "fc_out", "out", "v_a", "0", "2"] {
            let wk = format!("{p}.weight");
            let bk = format!("{p}.bias");
            if !dqn_vb.contains_tensor(&wk) || !dqn_vb.contains_tensor(&bk) {
                continue;
            }
            let w = dqn_vb.get_unchecked_dtype(&wk, DType::F32)?;
            if w.dims().len() == 2 && w.dims()[0] == action_space {
                let feat_dim = w.dims()[1];
                let b = dqn_vb
                    .get_unchecked_dtype(&bk, DType::F32)?
                    .reshape((1, action_space))?;
                return Ok((
                    head_fc1,
                    OutputHead::Direct {
                        weight: w,
                        bias: b,
                        feat_dim,
                    },
                ));
            }
        }

        let _ = conv_channels;
        Err(candle_core::Error::Msg(format!(
            "No Mortal DQN output head found in the 'current_dqn' scope for a \
{action_space}-action model. Expected a [{}x F] dueling matrix, a v_head/a_head \
pair, or a [{action_space} x F] policy head.",
            action_space + 1
        )))
    }
}

pub fn mortal_action_index(action: &Action, num_players: u8) -> Option<usize> {
    if num_players == 3 {
        mortal_action_index_3p(action)
    } else {
        mortal_action_index_4p(action)
    }
}

fn mortal_action_index_4p(action: &Action) -> Option<usize> {
    match action.action_type {
        ActionType::Discard => {
            let tid = action.tile?;
            match tid {
                16 => Some(34),
                52 => Some(35),
                88 => Some(36),
                _ => Some(tile34(tid).min(33)),
            }
        }
        ActionType::Riichi => Some(37),
        ActionType::Chi => {
            let called_t34 = tile34(action.tile?);
            let mut c: Vec<usize> = action.consume_tiles.iter().map(|&t| tile34(t)).collect();
            c.sort_unstable();
            if c.len() >= 2 {
                if called_t34 < c[0] {
                    Some(38)
                } else if called_t34 > c[1] {
                    Some(40)
                } else {
                    Some(39)
                }
            } else {
                Some(38)
            }
        }
        ActionType::Pon => Some(41),
        ActionType::Daiminkan | ActionType::Ankan | ActionType::Kakan => Some(42),
        ActionType::Tsumo | ActionType::Ron => Some(43),
        ActionType::KyushuKyuhai => Some(44),
        ActionType::Pass => Some(45),
        ActionType::Kita => None,
    }
}

/// Action index for 3-player (sanma).
///
/// The layout is confirmed against the sanma fork's own documentation and mask
/// construction (`libriichi/src/consts.rs` lists discard+aka, then riichi,
/// nukidora, pon, kan, agari, ryukyoku, pass = 44, and `encode_obs` sets
/// `mask[38] = nukidora`, `mask[39] = pon`, `mask[40] = kan`,
/// `mask[41] = agari`, `mask[42] = ryukyoku`, `mask[43] = pass`).
///
/// Chi does not exist in sanma, so no slot maps to it.
fn mortal_action_index_3p(action: &Action) -> Option<usize> {
    match action.action_type {
        ActionType::Discard => {
            let tid = action.tile?;
            match tid {
                16 => Some(34), // 5mr
                52 => Some(35), // 5pr
                88 => Some(36), // 5sr
                // Sanma keeps Mortal's full 34-wide tile axis, so a discard's
                // slot is the tile's own index. Akagi's *own* 3p model uses a
                // separate 27-wide compaction for manzu, which must not be
                // applied to a Mortal sanma model.
                _ => Some(tile34(tid).min(33)),
            }
        }
        ActionType::Riichi => Some(37),
        ActionType::Kita => Some(38), // nukidora (抜きドラ)
        ActionType::Pon => Some(39),
        ActionType::Daiminkan | ActionType::Ankan | ActionType::Kakan => Some(40),
        ActionType::Tsumo | ActionType::Ron => Some(41),
        ActionType::KyushuKyuhai => Some(42),
        ActionType::Pass => Some(43),
        ActionType::Chi => None,
    }
}

/// Rank the legal actions by Mortal's dueling-DQN Q value.
///
/// Reproduces `DQN.forward` + `MortalEngine.evaluate` from upstream:
///
/// ```text
/// v, a   = net(phi).split((1, ACTION_SPACE), dim=-1)
/// a_sum  = a.masked_fill(~mask, 0.).sum(-1, keepdim=True)
/// a_mean = a_sum / mask.sum(-1, keepdim=True)      # over LEGAL actions only
/// q      = (v + a - a_mean).masked_fill(~mask, -inf)
/// action = q.argmax(-1)                            # greedy
/// ```
///
/// Two details that are easy to get wrong and that change the chosen action:
///
/// * `a_mean` is the mean advantage over the **legal** actions, not over all 46
///   slots. Averaging over the whole space shifts every `q` by a constant that
///   differs between positions, so it is not a no-op.
/// * The head is a *dueling* split, so `logits[0]` is the state value and
///   `logits[1..=46]` are the advantages. Treating the raw output as a policy
///   logit vector and softmaxing it (as an earlier revision did) computes a
///   different function entirely.
///
/// The returned probability is a softmax over the legal `q` values, which is
/// what the UI wants for display. Mortal itself plays `argmax`; callers that
/// need the exact policy should take the first element.
pub fn rank_by_mortal_logits(
    legal: &[Action],
    logits: &[f32],
    num_players: u8,
    top_n: usize,
) -> Vec<(Action, f32)> {
    let action_space = if num_players == 3 {
        MORTAL_ACTION_SPACE_3P
    } else {
        MORTAL_ACTION_SPACE_4P
    };

    // Duelling head: split into value and advantages only when the caller
    // handed us a raw head output. Both live paths already combine the head
    // (`MortalPthModel::forward_logits` does it in Rust, the ONNX host does it
    // in `forward_external_custom`), so a buffer of exactly `action_space`
    // values is per-action Q and must NOT be re-split - treating `q[0]` as a
    // state value and the rest as advantages would misread every action.
    let (value, advantages): (f32, &[f32]) = if logits.len() == action_space + 1 {
        (logits[0], &logits[1..])
    } else {
        (0.0f32, &logits[..])
    };
    let raw_head = logits.len() == action_space + 1;

    // Map each legal action to its slot, resolving the aka/plain collision in
    // favour of whichever the model scores higher (the mask allows only one
    // slot per physical discard, and the aka is a distinct action upstream).
    let mut by_id: BTreeMap<usize, &Action> = BTreeMap::new();
    for a in legal {
        let Some(idx) = mortal_action_index(a, num_players) else {
            continue;
        };
        if idx >= action_space {
            continue;
        }
        match by_id.entry(idx) {
            Entry::Vacant(e) => {
                e.insert(a);
            }
            Entry::Occupied(mut e) => {
                let prev_aka =
                    e.get().action_type == ActionType::Discard && e.get().tile.is_some_and(is_aka);
                let curr_aka =
                    a.action_type == ActionType::Discard && a.tile.is_some_and(is_aka);
                if prev_aka && !curr_aka {
                    e.insert(a);
                }
            }
        }
    }
    if by_id.is_empty() {
        return Vec::new();
    }

    // a_mean over the legal slots only, and only when we split a raw head.
    let a_mean = if raw_head {
        let a_sum: f32 = by_id
            .keys()
            .map(|&i| advantages.get(i).copied().unwrap_or(0.0))
            .sum();
        a_sum / by_id.len() as f32
    } else {
        0.0
    };

    let mut scored: Vec<(&Action, f32)> = by_id
        .into_iter()
        .map(|(i, a)| {
            let adv = advantages.get(i).copied().unwrap_or(f32::NEG_INFINITY);
            (a, value + adv - a_mean)
        })
        .collect();

    let max_q = scored
        .iter()
        .map(|(_, q)| *q)
        .fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = scored.iter().map(|(_, q)| (q - max_q).exp()).collect();
    let sum: f32 = exps.iter().sum();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let keep = top_n.min(scored.len());
    let order: Vec<usize> = (0..keep).collect();

    order
        .into_iter()
        .map(|i| {
            let prob = if sum > 0.0 { (scored[i].1 - max_q).exp() / sum } else { 0.0 };
            (scored[i].0.clone(), prob)
        })
        .collect()
}

/// Encode a 4-player position into Mortal's observation, plus the legal actions.
///
/// Uses [`crate::mortal_obs::encode_mortal_obs`], the libriichi-parity encoder,
/// which is verified bit-for-bit against the reference in
/// `tests/mortal_obs_diff.rs`. `in_channels` selects the layout version: only
/// the 1012-wide v4 layout is implemented, so any other width falls back to the
/// legacy Akagi-style planes rather than silently feeding the model a buffer
/// whose layout does not match its weights.
pub fn encode_mortal_obs_4p(
    state: &mut GameState,
    seat: u8,
    in_channels: usize,
) -> (Vec<f32>, Vec<Action>) {
    let obs = state.get_observation(seat);
    let legal = obs.legal_actions_method();

    if in_channels == crate::mortal_obs::CHANNELS_V4 {
        let view = crate::state_features::MortalSeatView::from_state(state, seat)
            .with_legal_actions(state, seat);
        return (crate::mortal_obs::encode_mortal_obs(&view), legal);
    }

    let turn = state.turn_count;
    let last_discard = state.last_discard.map(|(_pid, tile)| tile);
    let inp = enc_input_4p(&obs, turn, last_discard);
    (encode_mortal_planes(&inp, in_channels), legal)
}

/// Encode a 3-player position into Mortal's observation, plus the legal actions.
///
/// Uses the `libriichi3p` layout (`obs_shape(4) = 775`) when `in_channels`
/// is 775, which is what every sanma Mortal checkpoint expects. Other widths
/// fall back to the legacy Akagi-style planes rather than feeding the model a
/// buffer whose layout disagrees with its weights.
pub fn encode_mortal_obs_3p(
    state: &mut GameState3P,
    seat: u8,
    in_channels: usize,
) -> (Vec<f32>, Vec<Action>) {
    let obs = state.get_observation(seat);
    let legal: Vec<Action> = obs.legal_actions_method().into_iter().map(|a| a.0).collect();

    if in_channels == crate::mortal_obs::CHANNELS_SANMA {
        let view = crate::state_features::MortalSeatView::from_state_3p(state, seat)
            .with_legal_actions_3p(state, seat);
        return (
            crate::mortal_obs::encode_mortal_obs_with(&view, crate::mortal_obs::Layout::SANMA),
            legal,
        );
    }

    let turn = state.turn_count;
    let kita: [u8; 3] = std::array::from_fn(|i| state.players[i].kita_tiles.len() as u8);
    let last_discard = state.last_discard.map(|(_pid, tile)| tile);
    let inp = enc_input_3p(&obs, turn, kita, last_discard);
    (encode_mortal_planes(&inp, in_channels), legal)
}

pub fn encode_mortal_planes(inp: &EncInput, in_channels: usize) -> Vec<f32> {
    let t = MORTAL_TILE_DIM;
    let mut buf = vec![0.0f32; in_channels * t];
    let mut ch = 0usize;

    macro_rules! next_ch {
        () => {{
            let c = ch;
            if c >= in_channels {
                return buf;
            }
            ch += 1;
            c
        }};
    }
    macro_rules! set_tile {
        ($c:expr, $tile34:expr, $val:expr) => {{
            let idx = $tile34;
            if $c < in_channels && idx < t {
                buf[$c * t + idx] = $val;
            }
        }};
    }
    macro_rules! broadcast {
        ($c:expr, $val:expr) => {{
            if $c < in_channels {
                let base = $c * t;
                for i in 0..t {
                    buf[base + i] = $val;
                }
            }
        }};
    }

    let mut hand_counts = [0u8; 34];
    let mut aka_in_hand = [false; 3];
    for &tid in &inp.hand {
        let idx = tile34(tid).min(33);
        hand_counts[idx] = hand_counts[idx].saturating_add(1);
        match tid {
            16 => aka_in_hand[0] = true,
            52 => aka_in_hand[1] = true,
            88 => aka_in_hand[2] = true,
            _ => {}
        }
    }
    for thresh in 1..=4u8 {
        let c = next_ch!();
        for idx in 0..34 {
            if hand_counts[idx] >= thresh {
                set_tile!(c, idx, 1.0);
            }
        }
    }
    for (k, &aka_t34) in [4usize, 13, 22].iter().enumerate() {
        let c = next_ch!();
        if aka_in_hand[k] {
            broadcast!(c, 1.0);
            set_tile!(c, aka_t34, 1.0);
        }
    }

    let c_drawn = next_ch!();
    if let Some(tid) = inp.drawn_tile {
        set_tile!(c_drawn, tile34(tid).min(33), 1.0);
    }
    let c_last = next_ch!();
    if let Some(tid) = inp.last_discard {
        set_tile!(c_last, tile34(tid).min(33), 1.0);
    }
    let c_waits = next_ch!();
    for &w34 in &inp.waits {
        set_tile!(c_waits, (w34 as usize).min(33), 1.0);
    }

    let c_dora_ind = next_ch!();
    // libriichi's `encode_tile_set` writes FOUR count rows (the 1st..4th
    // occurrence of each tile) followed by THREE red-five rows, advancing seven
    // channels in total - not the two "indicator + derived dora" planes an
    // earlier revision used.
    let c_dora_aka = next_ch!();
    for &tid in &inp.dora_indicators {
        let ind34 = tile34(tid).min(33);
        // Count rows: place the nth copy in the nth row.
        for row in 0..4usize {
            let c = c_dora_ind + row;
            if c < in_channels {
                let idx = c * t + ind34;
                if buf[idx] == 0.0 {
                    buf[idx] = 1.0;
                    break;
                }
            }
        }
        // Aka rows: 5m / 5p / 5s occupy the next three channels.
        if is_aka(tid) {
            let aka_row = match ind34 {
                4 => 0usize,
                13 => 1,
                22 => 2,
                _ => 3,
            };
            if aka_row < 3 {
                let c = c_dora_aka + aka_row;
                if c < in_channels {
                    for i in 0..t {
                        buf[c * t + i] = 1.0;
                    }
                }
            }
        }
    }

    let c_rw = next_ch!();
    set_tile!(c_rw, 27 + (inp.round_wind as usize).min(3), 1.0);
    let c_sw = next_ch!();
    set_tile!(c_sw, 27 + (inp.seat_wind as usize).min(3), 1.0);

    let c_honba = next_ch!();
    broadcast!(c_honba, (inp.honba as f32 / 10.0).min(1.0));
    let c_sticks = next_ch!();
    broadcast!(c_sticks, (inp.riichi_sticks as f32 / 10.0).min(1.0));
    let c_turn = next_ch!();
    broadcast!(c_turn, (inp.turn_count as f32 / 70.0).min(1.0));
    let c_kyoku = next_ch!();
    broadcast!(c_kyoku, (inp.kyoku_index as f32 / 8.0).min(1.0));
    let c_dealer = next_ch!();
    broadcast!(c_dealer, if inp.is_dealer { 1.0 } else { 0.0 });
    let c_tenpai = next_ch!();
    broadcast!(c_tenpai, if inp.is_tenpai { 1.0 } else { 0.0 });

    for seat in &inp.seats {
        let c_score = next_ch!();
        broadcast!(c_score, (seat.score as f32 / 100_000.0).clamp(0.0, 2.0));
        let c_riichi = next_ch!();
        broadcast!(c_riichi, if seat.riichi_declared { 1.0 } else { 0.0 });
        let c_rtile = next_ch!();
        if let Some(tid) = seat.riichi_tile {
            set_tile!(c_rtile, tile34(tid).min(33), 1.0);
        }
        let c_kita = next_ch!();
        broadcast!(c_kita, (seat.kita_count as f32 / 4.0).min(1.0));

        let mut meld_counts = [0u8; 34];
        for &tid in &seat.meld_tiles {
            let idx = tile34(tid).min(33);
            meld_counts[idx] = meld_counts[idx].saturating_add(1);
        }
        for thresh in 1..=4u8 {
            let c = next_ch!();
            for idx in 0..34 {
                if meld_counts[idx] >= thresh {
                    set_tile!(c, idx, 1.0);
                }
            }
        }

        let mut disc_counts = [0u8; 34];
        for &tid in &seat.discards {
            let idx = tile34(tid).min(33);
            disc_counts[idx] = disc_counts[idx].saturating_add(1);
        }
        for thresh in 1..=4u8 {
            let c = next_ch!();
            for idx in 0..34 {
                if disc_counts[idx] >= thresh {
                    set_tile!(c, idx, 1.0);
                }
            }
        }
        for step in 0..24usize {
            let c = next_ch!();
            if let Some(&tid) = seat.discards.get(step) {
                set_tile!(c, tile34(tid).min(33), 1.0);
            }
        }
    }

    buf
}

#[cfg(test)]
mod mortal_activation_tests {
    //! Numeric checks on the activation and BatchNorm choices.
    //!
    //! `mortal/model.py` uses `nn.Mish` for v2..=v4 (not SiLU or ReLU) and
    //! overrides BatchNorm `eps` to 1e-3 for v3/v4. Both are easy to get
    //! subtly wrong in a hand-written port, and both change the logits without
    //! changing any shape.

    use super::*;

    /// `mish(x) = x * tanh(softplus(x))`, checked against known values.
    #[test]
    fn mish_matches_reference_values() {
        let dev = Device::Cpu;
        let xs = [-8.0f32, -2.0, -0.5, 0.0, 0.5, 2.0, 8.0];
        let t = Tensor::from_slice(&xs, (1, xs.len()), &dev).unwrap();
        let got = mish(&t).unwrap().to_vec2::<f32>().unwrap().remove(0);

        // Computed with Python: x * tanh(log1p(exp(x)))
        let want = [
            -0.002_683_3f32,
            -0.252_501_5,
            -0.220_743_8,
            0.0,
            0.375_245_2,
            1.943_959_0,
            7.999_998_2,
        ];
        for (i, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert!(
                (g - w).abs() < 1e-4,
                "mish({}) = {g}, expected {w}",
                xs[i]
            );
        }
    }

    #[test]
    fn activation_and_eps_follow_the_version_split() {
        // v1: ReLU + eps 1e-5. v2..=v4: Mish + eps 1e-3.
        assert_eq!(Actv::for_version(1), Actv::Relu);
        assert_eq!(Actv::for_version(2), Actv::Mish);
        assert_eq!(Actv::for_version(3), Actv::Mish);
        assert_eq!(Actv::for_version(4), Actv::Mish);

        assert_eq!(norm_eps(Actv::Relu), 1e-5);
        assert_eq!(norm_eps(Actv::Mish), 1e-3);

        // Only v1 uses the post-activation block layout.
        assert!(!is_pre_actv(1));
        for v in 2..=4u8 {
            assert!(is_pre_actv(v), "v{v} must be pre-activation");
        }
    }
}

#[cfg(test)]
mod mortal_layout_tests {
    //! Structural tests pinning the real Mortal checkpoint layout.
    //!
    //! The reference checkpoint (`model_v4_20240308_best_min.pth`) nests the
    //! trunk under `mortal.encoder.net.*` and the head under
    //! `current_dqn.net.*`. These tests lock the *names* the loader must
    //! recognise, so a future edit cannot quietly regress to the old
    //! `entry.0` / `blocks.N` guesses that never matched a real model.

    use super::*;

    /// Build a VarBuilder-free key set describing the reference architecture.
    fn reference_keys() -> Vec<String> {
        let mut keys = vec![
            "0.weight".to_string(),
            "0.bias".to_string(),
        ];
        for n in 1..=54usize {
            for sub in ["0", "3"] {
                for t in ["weight", "bias", "running_mean", "running_var", "num_batches_tracked"] {
                    keys.push(format!("{n}.res_unit.{sub}.{t}"));
                }
            }
            keys.push(format!("{n}.res_unit.2.weight"));
            keys.push(format!("{n}.res_unit.5.weight"));
            keys.push(format!("{n}.ca.shared_mlp.0.weight"));
            keys.push(format!("{n}.ca.shared_mlp.0.bias"));
            keys.push(format!("{n}.ca.shared_mlp.2.weight"));
            keys.push(format!("{n}.ca.shared_mlp.2.bias"));
        }
        keys.push("55.weight".to_string());
        keys.push("55.running_mean".to_string());
        keys.push("57.weight".to_string());
        keys.push("60.weight".to_string());
        keys
    }

    #[test]
    fn block_prefixes_cover_real_mortal_namespace() {
        let keys = reference_keys();
        // Block ordinal 0 must resolve to `1.res_unit` (entry conv is `0`).
        assert!(keys.contains(&"1.res_unit.2.weight".to_string()));
        // The loader's first candidate for block 0 is the bare `{N+1}.res_unit`
        // form, which is what applies once open_brain_vb has descended.
        let ordinal = 0usize;
        let candidates = [
            format!("{}.res_unit", ordinal + 1),
            format!("encoder.net.{}.res_unit", ordinal + 1),
        ];
        assert!(
            candidates.iter().any(|p| keys.iter().any(|k| k.starts_with(&format!("{p}.")))),
            "no block prefix candidate matched the reference namespace"
        );
    }

    #[test]
    fn channel_attention_is_a_sibling_of_res_unit() {
        let keys = reference_keys();
        // `ca` lives at `<N>.ca...`, NOT `<N>.res_unit.ca...`.
        assert!(keys.contains(&"1.ca.shared_mlp.0.weight".to_string()));
        let ca_layouts = [
            ("ca.shared_mlp.0", "ca.shared_mlp.2"),
            ("ca.fc1", "ca.fc2"),
        ];
        // The block prefix is `1.res_unit`; the attention lookup happens on the
        // parent (`1`), so the keys that must exist are `1.ca...`.
        assert!(
            ca_layouts.iter().any(|(a, b)| keys.contains(&format!("1.{a}.weight"))
                && keys.contains(&format!("1.{b}.weight"))),
            "no channel-attention layout matched the reference namespace"
        );
    }

    #[test]
    fn tail_modules_follow_the_block_run() {
        let keys = reference_keys();
        let has_res_unit = |i: usize| keys.contains(&format!("{i}.res_unit.2.weight"));
        let mut last_block = 0usize;
        let mut i = 1usize;
        while i < 256 && has_res_unit(i) {
            last_block = i;
            i += 1;
        }
        assert_eq!(last_block, 54, "expected 54 res blocks");

        // The tail is the post-BN, the reduction conv and the final Linear.
        assert!(keys.contains(&"55.running_mean".to_string()));
        assert!(keys.contains(&"57.weight".to_string()));
        assert!(keys.contains(&"60.weight".to_string()));
    }

    #[test]
    fn flattened_width_matches_reduction_conv() {
        // net.57 reduces 256 -> 32 channels; 32 * 34 tile columns = 1088, which
        // must equal net.60's input width, and net.60 outputs 1024, which must
        // equal the head's input width.
        let reduction_out = 32usize;
        let flat_width = reduction_out * MORTAL_TILE_DIM;
        let fc_in = 1088usize;
        let fc_out = 1024usize;
        let head_in = 1024usize;
        assert_eq!(flat_width, fc_in);
        assert_eq!(fc_out, head_in);
    }

    #[test]
    fn head_out_dim_is_action_space_plus_one() {
        // current_dqn.net.weight is (47, 1024): 46 actions plus the dueling
        // value head, which load_output_head must recognise as DuelingCombined.
        let head_out = 47usize;
        assert_eq!(head_out, MORTAL_ACTION_SPACE_4P + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mortal_action_mapping_4p_and_3p() {
        let aka_5m = Action::new(ActionType::Discard, Some(16), vec![], Some(0));
        let plain_5m = Action::new(ActionType::Discard, Some(17), vec![], Some(0));
        assert_eq!(mortal_action_index(&aka_5m, 4), Some(34));
        assert_eq!(mortal_action_index(&plain_5m, 4), Some(4));

        let chi_low = Action::new(ActionType::Chi, Some(0), vec![4, 8], Some(0));
        let chi_mid = Action::new(ActionType::Chi, Some(4), vec![0, 8], Some(0));
        let chi_high = Action::new(ActionType::Chi, Some(8), vec![0, 4], Some(0));
        assert_eq!(mortal_action_index(&chi_low, 4), Some(38));
        assert_eq!(mortal_action_index(&chi_mid, 4), Some(39));
        assert_eq!(mortal_action_index(&chi_high, 4), Some(40));

        let pass = Action::new(ActionType::Pass, None, vec![], Some(0));
        let kita = Action::new(ActionType::Kita, None, vec![], Some(0));
        assert_eq!(mortal_action_index(&pass, 4), Some(45));
        assert_eq!(mortal_action_index(&pass, 3), Some(43));
        // Sanma: 37 riichi, 38 nukidora, 39 pon, 40 kan, 41 agari, 42 ryukyoku,
        // 43 pass. Chi has no slot at all.
        assert_eq!(mortal_action_index(&kita, 3), Some(38));
    }

    /// The sanma action slots, verified against the sanma fork's own mask
    /// construction. Getting these permuted still produces legal play (the
    /// engine maps the index back), but the model's advice lands on the wrong
    /// action.
    #[test]
    fn mortal_action_mapping_3p_slots() {
        let riichi = Action::new(ActionType::Riichi, None, vec![], Some(0));
        let kita = Action::new(ActionType::Kita, None, vec![], Some(0));
        let pon = Action::new(ActionType::Pon, Some(4), vec![5, 6], Some(0));
        let daiminkan = Action::new(ActionType::Daiminkan, Some(4), vec![5, 6, 7], Some(0));
        let ankan = Action::new(ActionType::Ankan, Some(4), vec![4, 5, 6, 7], Some(0));
        let kakan = Action::new(ActionType::Kakan, Some(4), vec![4, 5, 6], Some(0));
        let tsumo = Action::new(ActionType::Tsumo, None, vec![], Some(0));
        let ron = Action::new(ActionType::Ron, Some(4), vec![], Some(0));
        let kyushu = Action::new(ActionType::KyushuKyuhai, None, vec![], Some(0));
        let pass = Action::new(ActionType::Pass, None, vec![], Some(0));
        let chi = Action::new(ActionType::Chi, Some(0), vec![4, 8], Some(0));

        assert_eq!(mortal_action_index(&riichi, 3), Some(37));
        assert_eq!(mortal_action_index(&kita, 3), Some(38), "nukidora");
        assert_eq!(mortal_action_index(&pon, 3), Some(39), "pon");
        assert_eq!(mortal_action_index(&daiminkan, 3), Some(40), "daiminkan");
        assert_eq!(mortal_action_index(&ankan, 3), Some(40), "ankan");
        assert_eq!(mortal_action_index(&kakan, 3), Some(40), "kakan");
        assert_eq!(mortal_action_index(&tsumo, 3), Some(41), "tsumo");
        assert_eq!(mortal_action_index(&ron, 3), Some(41), "ron");
        assert_eq!(mortal_action_index(&kyushu, 3), Some(42), "kyuushu kyuuhai");
        assert_eq!(mortal_action_index(&pass, 3), Some(43), "pass");
        assert_eq!(mortal_action_index(&chi, 3), None, "no chi in sanma");

        // Every slot must sit inside the 44-wide sanma action space.
        for a in [&riichi, &kita, &pon, &daiminkan, &ankan, &kakan, &tsumo, &ron, &kyushu, &pass] {
            let i = mortal_action_index(a, 3).unwrap();
            assert!(i < MORTAL_ACTION_SPACE_3P, "{:?} -> {i} out of range", a.action_type);
        }
    }

    /// Sanma discards use the full 34-wide tile axis: 2m..8m must all map to
    /// their own slots, and the aka fives occupy 34..=36 alongside them.
    #[test]
    fn mortal_sanma_discards_use_the_full_tile_axis() {
        // 2m (raw 4) is index 1; 8m (raw 28) is index 7.
        for (raw, want) in [(0u8, 0usize), (4, 1), (28, 7), (32, 8), (36, 9), (108, 27), (132, 33)] {
            let a = Action::new(ActionType::Discard, Some(raw), vec![], Some(0));
            assert_eq!(
                mortal_action_index(&a, 3),
                Some(want),
                "raw {raw} should be slot {want} in sanma"
            );
        }

        // Aka fives keep their dedicated slots, and 5m's plain copy stays at 4.
        for (raw, want) in [(16u8, 34usize), (52, 35), (88, 36), (17, 4)] {
            let a = Action::new(ActionType::Discard, Some(raw), vec![], Some(0));
            assert_eq!(mortal_action_index(&a, 3), Some(want), "raw {raw} -> {want}");
        }
    }

    #[test]
    fn mortal_ranking_normalizes_and_orders() {
        let legal = vec![
            Action::new(ActionType::Discard, Some(0), vec![], Some(0)),
            Action::new(ActionType::Pon, Some(4), vec![5, 6], Some(0)),
            Action::new(ActionType::Pass, None, vec![], Some(0)),
        ];
        let mut logits = vec![0.0f32; 46];
        logits[0] = 0.5;
        logits[41] = 2.5;
        logits[45] = 1.0;

        let ranked = rank_by_mortal_logits(&legal, &logits, 4, 3);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].0.action_type, ActionType::Pon);
        assert_eq!(ranked[1].0.action_type, ActionType::Pass);
        assert_eq!(ranked[2].0.action_type, ActionType::Discard);
        let sum: f32 = ranked.iter().map(|(_, p)| *p).sum();
        assert!((sum - 1.0).abs() < 1e-5);
    }

    /// The v4 head is a dueling split: `logits[0]` is the state value and
    /// `logits[1..]` are per-action advantages. The advantage mean is taken
    /// over the *legal* actions only.
    #[test]
    fn mortal_dueling_head_uses_the_legal_advantage_mean() {
        // 46 actions + 1 value slot.
        let mut logits = vec![0.0f32; 47];
        logits[0] = 10.0; // v
        logits[1] = 1.0; // action slot 0 (1m discard)
        logits[42] = 5.0; // pon slot 41
        logits[46] = 2.0; // pass slot 45

        let legal = vec![
            Action::new(ActionType::Discard, Some(0), vec![], Some(0)),
            Action::new(ActionType::Pon, Some(4), vec![5, 6], Some(0)),
            Action::new(ActionType::Pass, None, vec![], Some(0)),
        ];

        // a_mean over the three legal slots = (1 + 5 + 2) / 3 = 8/3.
        let a_mean = 8.0f32 / 3.0;
        let q_pon = 10.0 + 5.0 - a_mean;
        let q_pass = 10.0 + 2.0 - a_mean;
        let q_disc = 10.0 + 1.0 - a_mean;

        let ranked = rank_by_mortal_logits(&legal, &logits, 4, 3);
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].0.action_type, ActionType::Pon, "q_pon is largest");
        assert_eq!(ranked[1].0.action_type, ActionType::Pass);
        assert_eq!(ranked[2].0.action_type, ActionType::Discard);

        // Relative order must follow the q values, which is invariant to the
        // shared a_mean shift - so verify the *spread* is preserved.
        assert!((q_pon - q_pass - 3.0).abs() < 1e-4);
        assert!((q_pass - q_disc - 1.0).abs() < 1e-4);
    }

    /// Averaging over the whole action space instead of the legal set would
    /// reorder the actions, so this pins that the illegal slots are excluded.
    #[test]
    fn illegal_slots_do_not_dilute_the_advantage_mean() {
        // Two actions with very different advantages, and 44 illegal slots with
        // a huge advantage that must be ignored.
        let mut logits = vec![0.0f32; 47];
        logits[0] = 0.0;
        logits[1] = 0.0; // discard 1m
        logits[46] = 4.0; // pass
        for slot in 2..46 {
            logits[slot] = 1000.0; // illegal, must not affect a_mean
        }

        let legal = vec![
            Action::new(ActionType::Discard, Some(0), vec![], Some(0)),
            Action::new(ActionType::Pass, None, vec![], Some(0)),
        ];

        // a_mean = (0 + 4) / 2 = 2 -> q_discard = -2, q_pass = +2.
        let ranked = rank_by_mortal_logits(&legal, &logits, 4, 2);
        assert_eq!(ranked.len(), 2);
        assert_eq!(
            ranked[0].0.action_type,
            ActionType::Pass,
            "pass has the higher legal advantage"
        );
        assert_eq!(ranked[1].0.action_type, ActionType::Discard);
    }

    /// Aka fives occupy their own action slots (34/35/36), matching libriichi's
    /// 37-wide `discard_candidates_aka` with the aka at 34..=36.
    #[test]
    fn aka_fives_get_their_own_action_slots() {
        let aka_5m = Action::new(ActionType::Discard, Some(16), vec![], Some(0));
        let plain_5m = Action::new(ActionType::Discard, Some(17), vec![], Some(0));
        let aka_5p = Action::new(ActionType::Discard, Some(52), vec![], Some(0));
        let aka_5s = Action::new(ActionType::Discard, Some(88), vec![], Some(0));

        assert_eq!(mortal_action_index(&aka_5m, 4), Some(34));
        assert_eq!(mortal_action_index(&aka_5p, 4), Some(35));
        assert_eq!(mortal_action_index(&aka_5s, 4), Some(36));
        // The plain five keeps its own 0..33 slot.
        assert_eq!(mortal_action_index(&plain_5m, 4), Some(4));
    }

    /// `make_action_payload`-style check: the aka action and its plain twin are
    /// distinct slots, so both can be legal at once and must not collapse.
    #[test]
    fn aka_and_plain_discards_are_distinct_slots() {
        let mut logits = vec![0.0f32; 47];
        // Give the plain five the higher advantage.
        logits[1 + 4] = 3.0;
        logits[1 + 34] = 1.0;
        logits[1 + 45] = 0.0;
        logits[0] = 0.0;

        let aka = Action::new(ActionType::Discard, Some(16), vec![], Some(0));
        let plain = Action::new(ActionType::Discard, Some(17), vec![], Some(0));
        let pass = Action::new(ActionType::Pass, None, vec![], Some(0));
        let legal = vec![aka, plain, pass];

        let ranked = rank_by_mortal_logits(&legal, &logits, 4, 3);
        assert_eq!(ranked.len(), 3, "both fives stay distinct");
        assert_eq!(ranked[0].0.tile, Some(17), "plain 5m wins on advantage");
    }
}
