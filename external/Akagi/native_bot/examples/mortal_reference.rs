use std::{fs, path::Path};
use native_bot::mortal::MortalPthModel;

fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(args.len() == 4, "usage: mortal_reference checkpoint input.json output.json");
    let model = MortalPthModel::load_from_file(Path::new(&args[1]))?;
    let input: Vec<f32> = serde_json::from_str(&fs::read_to_string(&args[2])?)?;
    let output = model.forward_logits(&input)?;
    anyhow::ensure!(output.iter().all(|v| v.is_finite()), "non-finite Q values");
    eprintln!("Mortal {:?}; Q range {:?}", model.meta,
        output.iter().copied().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo,hi),v| (lo.min(v),hi.max(v))));
    fs::write(&args[3], serde_json::to_string(&output)?)?;
    Ok(())
}
