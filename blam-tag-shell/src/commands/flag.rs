use anyhow::{Context, Result};
use blam_tags::TagFile;

pub fn run(
    file: &str,
    path: &str,
    flag_name: &str,
    action: Option<&str>,
    output: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let mut tag = TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;

    let new_value = {
        let mut root = tag.root_mut();
        let mut field = root
            .field_path_mut(path)
            .with_context(|| format!("field '{}' not found", path))?;

        let mut flag = field
            .flag_mut(flag_name)
            .with_context(|| format!("flag '{}' not found on field '{}'", flag_name, path))?;

        match action {
            None => {
                println!("{path}.{flag_name} = {}", if flag.is_set() { "on" } else { "off" });
                return Ok(());
            }
            Some("on") => { flag.set(true); true }
            Some("off") => { flag.set(false); false }
            Some("toggle") => flag.toggle(),
            Some(other) => anyhow::bail!("unknown action '{}' (expected on, off, toggle)", other),
        }
    };

    if dry_run {
        println!(
            "(dry run) would set {path}.{flag_name} = {}",
            if new_value { "on" } else { "off" },
        );
        return Ok(());
    }

    let out_path = output.unwrap_or(file);
    tag.write(out_path)
        .map_err(|e| anyhow::anyhow!("failed to save tag file: {e}"))?;
    println!("{path}.{flag_name} = {}", if new_value { "on" } else { "off" });
    if out_path != file {
        println!("saved to {out_path}");
    }

    Ok(())
}
