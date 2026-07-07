//! The `binary` strategy: download a prebuilt archive for the current
//! OS/arch and copy the binary it contains into porta's `bin/`.

use crate::archive;
use crate::install::{binary_file_name, make_executable, Outcome, Strategy};
use crate::manifest::{self, ArchiveKind, BinarySpec, Tool};
use anyhow::{Context, Result};

pub fn install(tool: &Tool, spec: &BinarySpec) -> Result<Outcome> {
    let target_key = manifest::current_target();
    let target = manifest::require_binary_target(spec, &target_key)?;

    println!(
        "porta: downloading `{}` {} for {target_key}",
        tool.label(),
        spec.version
    );

    crate::paths::ensure_layout()?;
    let cache_dir = crate::paths::cache_dir()
        .join(&tool.name)
        .join(&spec.version);
    let archive_file_name = target
        .url
        .rsplit('/')
        .next()
        .unwrap_or("download")
        .to_string();
    let archive_path = cache_dir.join(&archive_file_name);

    if !archive_path.exists() {
        crate::download::download_to_file(&target.url, &archive_path)
            .with_context(|| format!("downloading {}", target.url))?;
    }

    let dest_bin = crate::paths::bin_dir().join(binary_file_name(&tool.name));

    match target.archive {
        ArchiveKind::Raw => {
            std::fs::create_dir_all(crate::paths::bin_dir())?;
            std::fs::copy(&archive_path, &dest_bin).with_context(|| {
                format!(
                    "copying {} to {}",
                    archive_path.display(),
                    dest_bin.display()
                )
            })?;
        }
        kind => {
            let extract_dir = cache_dir.join("extracted");
            if extract_dir.exists() {
                std::fs::remove_dir_all(&extract_dir)?;
            }
            archive::extract(&archive_path, kind, &extract_dir)
                .with_context(|| format!("extracting {}", archive_path.display()))?;
            let found = archive::locate(&extract_dir, &target.binary_path)?;
            std::fs::create_dir_all(crate::paths::bin_dir())?;
            std::fs::copy(&found, &dest_bin).with_context(|| {
                format!("copying {} to {}", found.display(), dest_bin.display())
            })?;
        }
    }

    make_executable(&dest_bin)?;

    Ok(Outcome {
        version: spec.version.clone(),
        strategy: Strategy::Binary,
        location: dest_bin.display().to_string(),
    })
}
