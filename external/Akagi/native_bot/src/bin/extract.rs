//! Offline training-data extractor.
//!
//! Reads mjai `.json.gz` game logs, replays each through riichienv-core, and
//! writes behavior-cloning samples `(obs, action, mask)` as compact binary
//! shards for the Python trainer. Observations are stored as `u8`
//! (value in `[0, 4]` scaled to `0..=255`, `obs_scale = 4.0`).
//!
//! Usage:
//!   extract <dataset_dir> <num_players> <out_dir> [max_games] [max_samples]
//!
//! Example (light/fast, 4p):
//!   extract dataset/p4 4 out/p4 20000 4000000

use std::fs::{self, File};
use std::io::{BufWriter, Read, Write};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use rayon::prelude::*;

use native_bot::action_codec::action_space;
use native_bot::obs::channels;
use native_bot::replay::replay_game;
use native_bot::tiles::tile_dim;

const OBS_SCALE: f32 = 4.0;

#[inline]
fn quantize(v: f32) -> u8 {
    ((v.clamp(0.0, OBS_SCALE) / OBS_SCALE) * 255.0).round() as u8
}

fn collect_gz(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read_dir {dir:?}"))? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_gz(&path, out)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("gz") {
            out.push(path);
        }
    }
    Ok(())
}

fn read_gz(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut raw = Vec::new();
    f.read_to_end(&mut raw)?;
    let mut gz = GzDecoder::new(&raw[..]);
    let mut s = String::new();
    gz.read_to_string(&mut s)?;
    Ok(s)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: {} <dataset_dir> <num_players> <out_dir> [max_games] [max_samples]",
            args[0]
        );
        std::process::exit(2);
    }
    let dataset_dir = PathBuf::from(&args[1]);
    let num_players: u8 = args[2].parse().context("num_players")?;
    let out_dir = PathBuf::from(&args[3]);
    let max_games: usize = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);
    let max_samples: usize = args
        .get(5)
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX);

    fs::create_dir_all(&out_dir)?;

    let mut files = Vec::new();
    collect_gz(&dataset_dir, &mut files)?;
    files.sort();
    if files.len() > max_games {
        files.truncate(max_games);
    }
    let n_files = files.len();
    println!(
        "extract: {n_files} games, {num_players}p, C={} T={} A={}, cap {max_samples} samples -> {out_dir:?}",
        channels(num_players),
        tile_dim(num_players),
        action_space(num_players),
    );

    let c = channels(num_players);
    let t = tile_dim(num_players);
    let a = action_space(num_players);
    let obs_len = c * t;

    // One shard per rayon chunk; keep shards ~balanced.
    let n_threads = rayon::current_num_threads().max(1);
    let n_shards = (n_threads * 4).min(n_files.max(1));
    let chunk_size = n_files.div_ceil(n_shards.max(1));
    let chunks: Vec<&[PathBuf]> = files.chunks(chunk_size.max(1)).collect();

    let total = AtomicUsize::new(0);

    let shard = |ci: usize, chunk: &[PathBuf]| -> Result<usize> {
        let create = |ext: &str| -> Result<BufWriter<File>> {
            let path = out_dir.join(format!("{ci}.{ext}"));
            let f = File::create(&path).with_context(|| format!("create {path:?}"))?;
            Ok(BufWriter::new(f))
        };
        let mut obs_w = create("obs")?;
        let mut act_w = create("act")?;
        let mut msk_w = create("msk")?;
        let mut local = 0usize;

        for path in chunk {
            if total.load(Ordering::Relaxed) >= max_samples {
                break;
            }
            let content = match read_gz(path) {
                Ok(s) => s,
                Err(_) => continue,
            };

            // Buffer the game's samples; only commit if replay didn't panic,
            // so a pathological log never writes a torn record.
            let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| {
                let mut g_obs: Vec<u8> = Vec::new();
                let mut g_act: Vec<u8> = Vec::new();
                let mut g_msk: Vec<u8> = Vec::new();
                let mut g_n = 0usize;
                {
                    let mut emit = |obs: &[f32], act: u16, mask: &[u8]| {
                        g_obs.extend(obs.iter().map(|&v| quantize(v)));
                        g_act.extend_from_slice(&act.to_le_bytes());
                        g_msk.extend_from_slice(mask);
                        g_n += 1;
                    };
                    replay_game(&content, num_players, &mut emit);
                }
                (g_obs, g_act, g_msk, g_n)
            }));

            if let Ok((g_obs, g_act, g_msk, g_n)) = outcome {
                // Sanity: lengths must match declared geometry.
                debug_assert_eq!(g_obs.len(), g_n * obs_len);
                debug_assert_eq!(g_msk.len(), g_n * a);
                // A write failure (full disk) would silently truncate a shard and
                // desync it from the sample counts in meta.json — fail loudly.
                obs_w.write_all(&g_obs).context("write obs shard")?;
                act_w.write_all(&g_act).context("write act shard")?;
                msk_w.write_all(&g_msk).context("write mask shard")?;
                local += g_n;
                total.fetch_add(g_n, Ordering::Relaxed);
            }
        }

        obs_w.flush().context("flush obs shard")?;
        act_w.flush().context("flush act shard")?;
        msk_w.flush().context("flush mask shard")?;
        Ok(local)
    };

    // Each game is guarded by `catch_unwind`; silence the default panic printer
    // for the duration of the parallel replay so rare pathological logs don't
    // spam stderr (they're skipped). Scoped: the hook is restored immediately
    // after, and every non-replay failure inside is a `Result`, not a panic — so
    // a read-only out_dir reports an error instead of exiting silently.
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let shard_counts: Result<Vec<usize>> = chunks
        .par_iter()
        .enumerate()
        .map(|(ci, chunk)| shard(ci, chunk))
        .collect();
    std::panic::set_hook(prev_hook);
    let shard_counts = shard_counts?;

    let total_samples: usize = shard_counts.iter().sum();
    let meta = serde_json::json!({
        "num_players": num_players,
        "channels": c,
        "tile_dim": t,
        "action_space": a,
        "obs_len": obs_len,
        "obs_scale": OBS_SCALE,
        "num_shards": chunks.len(),
        "shard_samples": shard_counts,
        "total_samples": total_samples,
    });
    fs::write(out_dir.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;

    println!(
        "extract: wrote {total_samples} samples across {} shards",
        chunks.len()
    );
    Ok(())
}
