use native_bot::{model::Model, Geometry};
use serde_json::json;
use std::{env, fs};

fn argument(flag: &str) -> String {
    let args: Vec<String> = env::args().collect();
    let index = args.iter().position(|value| value == flag)
        .unwrap_or_else(|| panic!("missing {flag}"));
    args.get(index + 1).cloned().unwrap_or_else(|| panic!("missing value for {flag}"))
}

fn main() -> anyhow::Result<()> {
    let weights = fs::read(argument("--weights"))?;
    let players = argument("--players").parse::<u8>()?;
    let geometry = Geometry::for_players(players);
    let observation: Vec<f32> = serde_json::from_slice(&fs::read(argument("--input"))?)?;
    anyhow::ensure!(observation.len() == geometry.channels * geometry.tile_dim,
        "observation length does not match player geometry");
    let model = Model::from_safetensors(weights, players)?;
    println!("{}", json!(model.forward_logits(&observation)?));
    Ok(())
}
