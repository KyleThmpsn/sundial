//! Building a newly registered patch-zero package that holds only authored tags.
use super::*;

/// Builds a newly registered patch-zero package containing only authored tags.
///
/// Unlike an overlay, this package owns its complete directory and every physical payload block.
/// It is intended for resource graphs whose runtime handles must be allocated from a fresh package
/// datum table rather than appended to a stock patch chain.
#[cfg(test)]
pub(crate) fn build_standalone_package_with_references(
    package_directory: &Path,
    package_id: u16,
    output_file_name: &str,
    new_tags: &[NewTagSpec],
    reference_overrides: &[NewTagReferenceOverride],
) -> AuthoringResult<ExtendedOverlayArtifact> {
    build_standalone_package_with_progress(
        package_directory,
        package_id,
        output_file_name,
        new_tags,
        reference_overrides,
        &mut |_| {},
    )
}

#[cfg(test)]
pub(crate) fn build_standalone_package_with_progress(
    package_directory: &Path,
    package_id: u16,
    output_file_name: &str,
    new_tags: &[NewTagSpec],
    reference_overrides: &[NewTagReferenceOverride],
    progress: &mut dyn FnMut(&str),
) -> AuthoringResult<ExtendedOverlayArtifact> {
    build_standalone_package_from(
        package_directory,
        None,
        package_id,
        output_file_name,
        new_tags,
        reference_overrides,
        progress,
    )
}

/// The same standalone package, with the package directory already scanned.
pub(crate) fn build_standalone_package_with_chains(
    chains: &PatchChains,
    package_id: u16,
    output_file_name: &str,
    new_tags: &[NewTagSpec],
    reference_overrides: &[NewTagReferenceOverride],
    progress: &mut dyn FnMut(&str),
) -> AuthoringResult<ExtendedOverlayArtifact> {
    build_standalone_package_from(
        chains.directory(),
        Some(chains),
        package_id,
        output_file_name,
        new_tags,
        reference_overrides,
        progress,
    )
}

fn build_standalone_package_from(
    package_directory: &Path,
    chains: Option<&PatchChains>,
    package_id: u16,
    output_file_name: &str,
    new_tags: &[NewTagSpec],
    reference_overrides: &[NewTagReferenceOverride],
    progress: &mut dyn FnMut(&str),
) -> AuthoringResult<ExtendedOverlayArtifact> {
    progress("Checking Package Identity");
    if new_tags.is_empty() {
        return Err(AuthoringError::InvalidInput(
            "A standalone package needs at least one authored tag".into(),
        ));
    }
    validate_payloads(&[], new_tags)?;
    if !is_authored_standalone_package_id(package_id) {
        return Err(AuthoringError::InvalidInput(format!(
            "Standalone authored package id {package_id:04X} is outside the untracked \
             {MIN_AUTHORED_STANDALONE_PACKAGE_ID:03X}..={MAX_AUTHORED_STANDALONE_PACKAGE_ID:03X} window"
        )));
    }
    ensure_package_id_unused(package_directory, package_id)?;
    let output_stem = output_file_name
        .strip_suffix("_0.pkg")
        .ok_or_else(|| {
            AuthoringError::InvalidInput(format!(
                "Standalone package filename {output_file_name:?} must end in _0.pkg"
            ))
        })?
        .to_owned();
    if !output_stem.starts_with("w64_")
        || !output_stem.ends_with(&format!("_{package_id:04x}"))
        || output_stem
            .bytes()
            .any(|byte| !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'))
    {
        return Err(AuthoringError::InvalidInput(format!(
            "Standalone package filename {output_file_name:?} does not match package {package_id:04X}"
        )));
    }
    if new_tags.len() > MAX_PACKAGE_ENTRY_COUNT {
        return Err(AuthoringError::InvalidInput(format!(
            "Standalone package {package_id:04X} would exceed its {MAX_PACKAGE_ENTRY_COUNT}-entry limit"
        )));
    }

    let reference_modes = resolve_reference_modes(new_tags.len(), reference_overrides)?;
    progress("Preparing Entries");
    let metadata = resolve_appended_metadata(
        package_directory,
        chains,
        package_id,
        0,
        new_tags,
        &reference_modes,
    )?;
    let shared_tag_enrollments =
        resolve_shared_tag_enrollments(package_id, 0, new_tags, &metadata)?;
    let packing = packed::plan(
        new_tags
            .iter()
            .zip(&metadata)
            .map(|(tag, entry)| (tag.payload.len(), entry.alignment())),
    )?;
    let block_count = packing.block_count;
    if block_count == 0 || block_count > MAX_BLOCK_COUNT {
        return Err(AuthoringError::InvalidInput(format!(
            "Standalone package {package_id:04X} needs {block_count} blocks; the supported range is 1..={MAX_BLOCK_COUNT}"
        )));
    }
    // Wwise bypasses package decompression. Every block touched by a medium,
    // including blocks shared with other resources, must remain raw. Full raw
    // blocks are written consecutively, so a spanning medium is contiguous.
    let mut streamed_blocks = vec![false; block_count];
    for ((tag, entry), location) in new_tags.iter().zip(&metadata).zip(&packing.locations) {
        if entry.is_streamed_media() {
            let span = location
                .offset
                .checked_add(tag.payload.len())
                .ok_or_else(|| invalid("Streamed payload span overflow"))?;
            let end = location
                .block
                .checked_add(span.div_ceil(BLOCK_SIZE))
                .ok_or_else(|| invalid("Streamed block range overflow"))?;
            streamed_blocks
                .get_mut(location.block..end)
                .ok_or_else(|| invalid("Streamed payload exceeds its block plan"))?
                .fill(true);
        }
    }
    let prefixes = metadata
        .iter()
        .map(|entry| entry.prefix)
        .collect::<Vec<_>>();
    let mut artifact = build_standalone_package_skeleton(
        package_id,
        &prefixes,
        block_count,
        &shared_tag_enrollments,
    )?;
    let layout = PackageLayout::parse(&artifact)?;
    let opaque_trailer = layout.opaque_trailer(&artifact)?.to_vec();
    artifact.truncate(layout.opaque_trailer_offset);

    let block_encoder = PackageBlockEncoder::open_for_packages(package_directory)?;
    let tag_allocator = AppendedTagAllocator::new(package_id, 0);
    progress("Encoding Blocks");
    let written_blocks = packed::visit_blocks(
        new_tags
            .iter()
            .zip(&metadata)
            .map(|(tag, entry)| (tag.payload.as_slice(), entry.alignment())),
        |index, chunk| {
            let encoded = if streamed_blocks[index] {
                EncodedPackageBlock::raw(chunk)?
            } else {
                block_encoder.encode(package_id, chunk)?
            };
            let payload_offset = append_aligned(&mut artifact, &encoded.stored);
            let block_row = layout.block_table_offset + index * BLOCK_HEADER_SIZE;
            write_block_header(&mut artifact, block_row, payload_offset, &encoded, 0)
        },
    )?;
    if written_blocks != block_count {
        return Err(validation(
            "Standalone package block allocation did not converge",
        ));
    }
    let mut appended_tags = Vec::with_capacity(new_tags.len());
    for (index, (spec, location)) in new_tags.iter().zip(&packing.locations).enumerate() {
        write_entry_location(
            &mut artifact,
            layout.entry_table_offset + index * ENTRY_HEADER_SIZE,
            location.block,
            location.offset,
            spec.payload.len(),
        )?;
        appended_tags.push(AppendedTag {
            tag: tag_allocator.assigned_tag(
                index,
                "Standalone package entry",
                "standalone appended tag",
            )?,
            template_tag: spec.template_tag,
            file_size: spec.payload.len(),
        });
    }
    progress("Finalizing Package Tables");
    layout.update_package_tables_hash(&mut artifact)?;
    append_opaque_trailer(&mut artifact, &opaque_trailer)?;
    layout.set_file_size(&mut artifact)?;
    let candidate_layout = PackageLayout::parse(&artifact).map_err(|error| {
        validation(format!(
            "The standalone package metadata could not be reopened: {error}"
        ))
    })?;
    if candidate_layout.package_id != package_id
        || candidate_layout.patch_id != 0
        || candidate_layout.entry_count != new_tags.len()
        || candidate_layout.block_count != block_count
        || candidate_layout.shared_tag_enrollment_count() != shared_tag_enrollments.len()
    {
        return Err(validation(
            "Standalone package identity, counts, or shared-tag enrollment changed",
        ));
    }

    progress("Validating Payloads");
    let virtual_path = package_directory.join(output_file_name);
    let virtual_path_text = virtual_path.to_str().ok_or_else(|| {
        AuthoringError::InvalidInput("The standalone package path is not valid Unicode".into())
    })?;
    let artifact = Bytes(Arc::new(artifact));
    let candidate = PackageD2PreBL::from_reader(virtual_path_text, Cursor::new(artifact.clone()))
        .map_err(|error| {
        validation(format!(
            "The standalone package could not be reopened by tiger-pkg: {error}"
        ))
    })?;
    for ((spec, appended), entry_metadata) in new_tags.iter().zip(&appended_tags).zip(&metadata) {
        let entry = candidate
            .entries()
            .get(appended.tag.entry_index() as usize)
            .ok_or_else(|| validation(format!("Standalone tag {} has no entry", appended.tag)))?;
        if entry.reference != entry_metadata.reference
            || entry.file_type != entry_metadata.file_type
            || entry.file_subtype != entry_metadata.file_subtype
            || entry.file_size as usize != spec.payload.len()
        {
            return Err(validation(format!(
                "Standalone tag {} has incorrect entry metadata",
                appended.tag
            )));
        }
        let payload = candidate.read_tag(appended.tag).map_err(|error| {
            validation(format!(
                "Could not read standalone tag {}: {error}",
                appended.tag
            ))
        })?;
        if payload != spec.payload {
            return Err(validation(format!(
                "Standalone tag {} did not round-trip",
                appended.tag
            )));
        }
    }

    let family = output_stem
        .strip_prefix("w64_")
        .and_then(|value| value.strip_suffix(&format!("_{package_id:04x}")))
        .ok_or_else(|| validation("Standalone package family could not be derived"))?;
    let chain = PatchChain {
        directory: package_directory.to_path_buf(),
        identity: PackageIdentity {
            platform: "w64".to_owned(),
            name: family.to_owned(),
            language: None,
            package_id,
            stem: output_stem,
        },
        files: vec![PatchFile {
            patch: 0,
            path: virtual_path,
            entry_count: new_tags.len(),
        }],
    };
    Ok(ExtendedOverlayArtifact {
        plan: ExtendedOverlayPlan {
            chain,
            output_file_name: output_file_name.to_owned(),
            original_entry_count: 0,
            append_start_entry_count: 0,
            reserved_entry_count: 0,
            final_entry_count: new_tags.len(),
            appended_tags,
        },
        bytes: artifact,
    })
}

fn ensure_package_id_unused(package_directory: &Path, package_id: u16) -> AuthoringResult<()> {
    for entry in fs::read_dir(package_directory)
        .map_err(|error| AuthoringError::io("list package directory", package_directory, error))?
    {
        let entry = entry.map_err(|error| {
            AuthoringError::io("read package directory entry", package_directory, error)
        })?;
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|error| AuthoringError::io("inspect package directory entry", &path, error))?
            .is_file()
            || !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pkg"))
        {
            continue;
        }
        let mut prefix = [0u8; 6];
        let mut file = File::open(&path)
            .map_err(|error| AuthoringError::io("open package header", &path, error))?;
        if let Err(error) = file.read_exact(&mut prefix) {
            return Err(AuthoringError::io("read package header", &path, error));
        }
        let candidate_id = u16::from_le_bytes([prefix[4], prefix[5]]);
        if candidate_id == package_id {
            return Err(AuthoringError::InvalidInput(format!(
                "Package id {package_id:04X} is already present at {}",
                path.display()
            )));
        }
    }
    Ok(())
}
