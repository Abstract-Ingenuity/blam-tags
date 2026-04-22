use anyhow::{Context, Result};
use blam_tags::fields::find_flag_bit;
use blam_tags::file::TagFile;
use blam_tags::path::lookup_mut;

pub fn run(
    file: &str,
    path: &str,
    flag_name: &str,
    action: Option<&str>,
    output: Option<&str>,
    dry_run: bool,
) -> Result<()> {
    let mut tag =
        TagFile::read(file).map_err(|e| anyhow::anyhow!("failed to load tag file: {e}"))?;

    let new_value = {
        let tag_stream = &mut tag.tag_stream;
        let layout = &tag_stream.layout.layout;

        let mut cursor = lookup_mut(layout, &mut tag_stream.data, path)
            .with_context(|| format!("field '{}' not found", path))?;

        let field = &layout.fields[cursor.field_index];
        let bit = find_flag_bit(layout, field, flag_name).with_context(|| {
            format!("flag '{}' not found on field '{}'", flag_name, path)
        })?;

        let mut parsed = cursor
            .parse(layout)
            .context("field has no parsed value (not a flags field?)")?;

        let current = parsed
            .flag_bit(bit)
            .with_context(|| format!("field '{}' is not a flags field", path))?;

        match action {
            None => {
                println!("{path}.{flag_name} = {}", if current { "on" } else { "off" });
                return Ok(());
            }
            Some(act) => {
                let new_value = match act {
                    "on" => true,
                    "off" => false,
                    "toggle" => !current,
                    other => anyhow::bail!(
                        "unknown action '{}' (expected on, off, toggle)",
                        other
                    ),
                };
                parsed.set_flag_bit(bit, new_value);
                cursor.set(layout, parsed);
                new_value
            }
        }
    };

    if dry_run {
        println!(
            "(dry run) would set {path}.{flag_name} = {}",
            if new_value { "on" } else { "off" }
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
