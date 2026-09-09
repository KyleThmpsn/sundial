#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> Result<(), Box<dyn std::error::Error>> {
    sundial::run(Box::new(parhelion::Parhelion::default()))?;
    Ok(())
}
