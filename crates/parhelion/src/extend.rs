use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Cursor, Read, Seek, SeekFrom, Write},
    mem::size_of,
    path::{Path, PathBuf},
};

use sha1::{Digest, Sha1};
use sundial::package_authoring::is_valid_package_tag;
use tiger_pkg::{Package, PackageD2PreBL, TagHash};

use crate::{
    AuthoringError, AuthoringResult, PackageIdentity, PatchChain, PatchFile,
    appended_tags::{AppendedTagAllocator, MAX_PACKAGE_ENTRY_COUNT},
    block_codec::{EncodedPackageBlock, PackageBlockEncoder},
    chain::discover_patch_chain,
    error::{invalid, validation},
    format::{
        BLOCK_HEADER_SIZE, BLOCK_SIZE, ENTRY_HEADER_SIZE, MAX_BLOCK_COUNT, PackageLayout,
        SHARED_TAG_COMPANION_CLASS, append_aligned, append_opaque_trailer,
        build_standalone_package_skeleton,
    },
    package_profile::{
        MAX_AUTHORED_STANDALONE_PACKAGE_ID, MIN_AUTHORED_STANDALONE_PACKAGE_ID,
        is_authored_standalone_package_id,
    },
};

const SHARED_TAG_OWNER_FILE_TYPE: u8 = 0x10;
const SHARED_TAG_COMPANION_FILE_TYPE: u8 = 0x08;
const SHARED_TAG_FILE_SUBTYPE: u8 = 0;

#[cfg(all(test, windows, target_pointer_width = "64"))]
mod compression_tests;
mod packed;

#[derive(Clone, Debug)]
pub struct ReplacementSpec {
    pub tag: TagHash,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct NewTagSpec {
    pub template_tag: TagHash,
    pub payload: Vec<u8>,
    pub storage: NewTagStorageMode,
}

/// Selects how an appended schema entry participates in the client's storage lifecycle.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum NewTagStorageMode {
    #[default]
    InheritTemplate,
}

/// Selects the reference stored in an appended package-entry header.
///
/// Appended ordinals are zero-based positions in the `new_tags` slice. They are resolved only
/// after the source entry count is known, so forward and reciprocal references are supported.
/// This low-level mechanism does not make a tag reachable from an investment or resource root.
/// Callers must author and validate that owning graph separately.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NewTagReference {
    /// Preserve the reference from the tag used as the appended entry's metadata template.
    Template,
    /// Reference another tag appended by the same overlay operation.
    Appended(usize),
}

/// Overrides the entry-header reference for one appended tag without changing [`NewTagSpec`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NewTagReferenceOverride {
    pub new_tag_ordinal: usize,
    pub reference: NewTagReference,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppendedTag {
    pub tag: TagHash,
    pub template_tag: TagHash,
    pub file_size: usize,
}

#[derive(Clone, Debug)]
pub struct ExtendedOverlayPlan {
    pub chain: PatchChain,
    pub output_file_name: String,
    /// Entry count in the latest source generation copied by the overlay.
    pub original_entry_count: usize,
    /// First index safe for newly authored tags after accounting for every prior generation.
    pub append_start_entry_count: usize,
    /// Retired historical indices represented by inert rows before the authored tail.
    pub reserved_entry_count: usize,
    pub final_entry_count: usize,
    pub appended_tags: Vec<AppendedTag>,
}

#[derive(Clone, Debug)]
pub struct ExtendedOverlayArtifact {
    pub plan: ExtendedOverlayPlan,
    bytes: Vec<u8>,
}

impl ExtendedOverlayArtifact {
    #[cfg(test)]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn write_new(&self, directory: &Path) -> AuthoringResult<PathBuf> {
        fs::create_dir_all(directory)
            .map_err(|error| AuthoringError::io("create staging directory", directory, error))?;
        let path = directory.join(&self.plan.output_file_name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| AuthoringError::io("create extended overlay", &path, error))?;
        file.write_all(&self.bytes)
            .map_err(|error| AuthoringError::io("write extended overlay", &path, error))?;
        file.sync_all()
            .map_err(|error| AuthoringError::io("flush extended overlay", &path, error))?;
        Ok(path)
    }
}

/// Builds a raw package overlay without proving that appended entries are rooted.
///
/// This is crate-internal on purpose. Product authoring paths must own the investment/resource
/// graph that makes each appended tag reachable.
#[cfg(test)]
pub(crate) fn build_extended_overlay(
    package_directory: &Path,
    package_id: u16,
    replacements: &[ReplacementSpec],
    new_tags: &[NewTagSpec],
) -> AuthoringResult<ExtendedOverlayArtifact> {
    build_extended_overlay_with_progress(
        package_directory,
        package_id,
        replacements,
        new_tags,
        &[],
        &mut |_| {},
    )
}

/// Builds an extended overlay while overriding selected appended-entry references.
///
/// Unspecified entry prefixes are preserved byte-for-byte. Each appended ordinal may be
/// overridden at most once.
#[cfg(test)]
pub(crate) fn build_extended_overlay_with_references(
    package_directory: &Path,
    package_id: u16,
    replacements: &[ReplacementSpec],
    new_tags: &[NewTagSpec],
    reference_overrides: &[NewTagReferenceOverride],
) -> AuthoringResult<ExtendedOverlayArtifact> {
    build_extended_overlay_with_progress(
        package_directory,
        package_id,
        replacements,
        new_tags,
        reference_overrides,
        &mut |_| {},
    )
}

/// Returns the first datum index that an overlay may safely assign in a package chain.
///
/// The latest generation's table can be shorter than an earlier generation's table. Returning
/// the historical high-water mark keeps native identities for those retired slots from being
/// repurposed as a different class of runtime object.
pub(crate) fn extended_overlay_append_start(
    package_directory: &Path,
    package_id: u16,
) -> AuthoringResult<usize> {
    Ok(discover_patch_chain(package_directory, package_id)?.historical_entry_count_high_water())
}

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

pub(crate) fn build_standalone_package_with_progress(
    package_directory: &Path,
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
    let metadata =
        resolve_appended_metadata(package_directory, package_id, 0, new_tags, &reference_modes)?;
    let shared_tag_enrollments =
        resolve_shared_tag_enrollments(package_id, 0, new_tags, &metadata)?;
    let packing = packed::plan(new_tags.iter().map(|tag| tag.payload.len()))?;
    let block_count = packing.block_count;
    if block_count == 0 || block_count > MAX_BLOCK_COUNT {
        return Err(AuthoringError::InvalidInput(format!(
            "Standalone package {package_id:04X} needs {block_count} blocks; the supported range is 1..={MAX_BLOCK_COUNT}"
        )));
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
        new_tags.iter().map(|tag| tag.payload.as_slice()),
        |index, chunk| {
            let encoded = block_encoder.encode(package_id, chunk)?;
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

pub(crate) fn build_extended_overlay_with_progress(
    package_directory: &Path,
    package_id: u16,
    replacements: &[ReplacementSpec],
    new_tags: &[NewTagSpec],
    reference_overrides: &[NewTagReferenceOverride],
    progress: &mut dyn FnMut(&str),
) -> AuthoringResult<ExtendedOverlayArtifact> {
    progress("Reading Source Package");
    if replacements.is_empty() && new_tags.is_empty() {
        return Err(AuthoringError::InvalidInput(
            "An extended overlay needs at least one replacement or new tag".into(),
        ));
    }
    validate_payloads(replacements, new_tags)?;
    let reference_modes = resolve_reference_modes(new_tags.len(), reference_overrides)?;
    let chain = discover_patch_chain(package_directory, package_id)?;
    let source_patch = chain.latest().patch;
    let target_patch = chain.next_patch()?;
    let output_file_name = chain.output_file_name()?;
    let source_path = &chain.latest().path;
    let source_path_text = source_path.to_str().ok_or_else(|| {
        AuthoringError::InvalidInput("The source package path is not valid Unicode".into())
    })?;
    let source_bytes = fs::read(source_path)
        .map_err(|error| AuthoringError::io("read source package", source_path, error))?;
    let source = PackageD2PreBL::open(source_path_text).map_err(|error| {
        AuthoringError::InvalidPackage(format!(
            "Could not open source package {}: {error}",
            source_path.display()
        ))
    })?;
    let layout = PackageLayout::parse(&source_bytes)?;
    let block_encoder = PackageBlockEncoder::open_for_packages(package_directory)?;
    if layout.package_id != package_id
        || layout.patch_id != u16::from(source_patch)
        || layout.entry_count != chain.latest().entry_count
    {
        return Err(AuthoringError::InvalidPackage(
            "Source package identity does not match its chain".into(),
        ));
    }
    let replacement_indices = validate_replacements(package_id, layout.entry_count, replacements)?;
    // Replacement-only overlays preserve the latest generation exactly. Once tags are appended,
    // however, every datum index used by an older generation remains reserved even if a later
    // stock patch shortened the entry table.
    let append_start_entry_count = if new_tags.is_empty() {
        layout.entry_count
    } else {
        chain.historical_entry_count_high_water()
    };
    let reserved_entry_count = append_start_entry_count
        .checked_sub(layout.entry_count)
        .ok_or_else(|| invalid("Historical package entry high-water mark regressed"))?;
    let inserted_entry_count = reserved_entry_count
        .checked_add(new_tags.len())
        .ok_or_else(|| invalid("Entry count overflowed"))?;
    let final_entry_count = layout
        .entry_count
        .checked_add(inserted_entry_count)
        .ok_or_else(|| invalid("Entry count overflowed"))?;
    if final_entry_count > MAX_PACKAGE_ENTRY_COUNT {
        return Err(AuthoringError::InvalidInput(format!(
            "Package {package_id:04x} has {} current entries, a historical high-water mark of \
             {append_start_entry_count}, and cannot append {} more",
            layout.entry_count,
            new_tags.len()
        )));
    }
    progress("Preparing Entries");
    let appended_metadata = resolve_appended_metadata(
        package_directory,
        package_id,
        append_start_entry_count,
        new_tags,
        &reference_modes,
    )?;
    let shared_tag_enrollments = resolve_shared_tag_enrollments(
        package_id,
        append_start_entry_count,
        new_tags,
        &appended_metadata,
    )?;
    let payload_block_counts = replacements
        .iter()
        .map(|spec| spec.payload.len().div_ceil(BLOCK_SIZE))
        .chain(
            new_tags
                .iter()
                .map(|spec| spec.payload.len().div_ceil(BLOCK_SIZE)),
        )
        .collect::<Vec<_>>();
    let added_block_count = payload_block_counts.iter().sum::<usize>();
    let final_block_count = layout
        .block_count
        .checked_add(added_block_count)
        .ok_or_else(|| invalid("Block count overflowed"))?;
    if final_block_count > MAX_BLOCK_COUNT {
        return Err(AuthoringError::InvalidInput(format!(
            "Package {package_id:04x} would exceed its {MAX_BLOCK_COUNT}-block limit"
        )));
    }

    let opaque_trailer = layout.opaque_trailer(&source_bytes)?.to_vec();
    let mut artifact = layout
        .sparse_overlay_metadata_prefix(&source_bytes)?
        .to_vec();
    let appended_entry_bytes = inserted_entry_count
        .checked_mul(ENTRY_HEADER_SIZE)
        .ok_or_else(|| invalid("Appended entry table size overflowed"))?;
    let entry_insert = layout
        .entry_count
        .checked_mul(ENTRY_HEADER_SIZE)
        .and_then(|size| layout.entry_table_offset.checked_add(size))
        .ok_or_else(|| invalid("Appended entry insertion offset overflowed"))?;
    artifact.splice(
        entry_insert..entry_insert,
        std::iter::repeat_n(0, appended_entry_bytes),
    );
    let final_entry_capacity = layout
        .entry_capacity
        .checked_add(inserted_entry_count)
        .ok_or_else(|| invalid("Final entry capacity overflowed"))?;
    let final_block_table_offset = final_entry_capacity
        .checked_mul(ENTRY_HEADER_SIZE)
        .and_then(|size| layout.entry_table_offset.checked_add(size))
        .and_then(|offset| offset.checked_add(layout.entry_table_trailer_size))
        .ok_or_else(|| invalid("Final block-table offset overflowed"))?;
    let block_insert = layout
        .block_count
        .checked_mul(BLOCK_HEADER_SIZE)
        .and_then(|size| final_block_table_offset.checked_add(size))
        .ok_or_else(|| invalid("Appended block insertion offset overflowed"))?;
    let appended_block_bytes = added_block_count
        .checked_mul(BLOCK_HEADER_SIZE)
        .ok_or_else(|| invalid("Appended block-table size overflowed"))?;
    artifact.splice(
        block_insert..block_insert,
        std::iter::repeat_n(0, appended_block_bytes),
    );
    layout.set_extended_counts(
        &mut artifact,
        target_patch,
        final_entry_count,
        final_block_count,
        final_block_table_offset,
        &shared_tag_enrollments,
    )?;

    // Inert rows keep retired historical indices occupied without restoring stale payloads or
    // assigning them a new runtime class. 0xFFFFFFFF is the stock package tombstone reference.
    write_reserved_entry_rows(
        &mut artifact,
        layout.entry_table_offset,
        layout.entry_count,
        append_start_entry_count,
    )?;

    for (new_index, metadata) in appended_metadata.iter().enumerate() {
        let index = append_start_entry_count
            .checked_add(new_index)
            .ok_or_else(|| invalid("Appended entry index overflowed"))?;
        let row = index
            .checked_mul(ENTRY_HEADER_SIZE)
            .and_then(|size| layout.entry_table_offset.checked_add(size))
            .ok_or_else(|| invalid("Appended entry row offset overflowed"))?;
        artifact[row..row + 8].copy_from_slice(&metadata.prefix);
    }

    let mut next_block = layout.block_count;
    let mut block_count_index = 0;
    progress("Encoding Blocks");
    for spec in replacements {
        write_payload(
            &mut artifact,
            &layout,
            final_block_table_offset,
            spec.tag.entry_index() as usize,
            &spec.payload,
            target_patch,
            &mut next_block,
            payload_block_counts[block_count_index],
            &block_encoder,
        )?;
        block_count_index += 1;
    }
    let mut appended_tags = Vec::with_capacity(new_tags.len());
    let tag_allocator = AppendedTagAllocator::new(package_id, append_start_entry_count);
    for (new_index, spec) in new_tags.iter().enumerate() {
        let index = append_start_entry_count
            .checked_add(new_index)
            .ok_or_else(|| invalid("Appended entry index overflowed"))?;
        let tag = tag_allocator.assigned_tag(
            new_index,
            "Overlay package entry",
            "overlay appended tag",
        )?;
        write_payload(
            &mut artifact,
            &layout,
            final_block_table_offset,
            index,
            &spec.payload,
            target_patch,
            &mut next_block,
            payload_block_counts[block_count_index],
            &block_encoder,
        )?;
        block_count_index += 1;
        appended_tags.push(AppendedTag {
            tag,
            template_tag: spec.template_tag,
            file_size: spec.payload.len(),
        });
    }
    if next_block != final_block_count {
        return Err(invalid(
            "Extended overlay block allocation did not converge",
        ));
    }
    progress("Finalizing Package Tables");
    layout.update_package_tables_hash(&mut artifact)?;
    append_opaque_trailer(&mut artifact, &opaque_trailer)?;
    layout.set_file_size(&mut artifact)?;
    let candidate_layout = PackageLayout::parse(&artifact).map_err(|error| {
        validation(format!(
            "The extended package metadata could not be reopened: {error}"
        ))
    })?;
    candidate_layout
        .sparse_overlay_metadata_prefix(&artifact)
        .map_err(|error| {
            validation(format!(
                "The extended package contains bytes outside its authored block store: {error}"
            ))
        })?;
    validate_shared_tag_enrollments(
        &source_bytes,
        &layout,
        &artifact,
        &candidate_layout,
        &shared_tag_enrollments,
    )?;

    progress("Validating Payloads");
    let output_path = package_directory.join(&output_file_name);
    let output_path_text = output_path.to_str().ok_or_else(|| {
        AuthoringError::InvalidInput("The output package path is not valid Unicode".into())
    })?;
    let candidate = PackageD2PreBL::from_reader(output_path_text, Cursor::new(artifact.clone()))
        .map_err(|error| {
            AuthoringError::Validation(format!(
                "The extended package could not be reopened: {error}"
            ))
        })?;
    validate_candidate(
        &source,
        &candidate,
        CandidateExpectations {
            replacements,
            new_specs: new_tags,
            appended: &appended_tags,
            appended_metadata: &appended_metadata,
            replacement_indices: &replacement_indices,
            append_start_entry_count,
            reserved_entry_count,
        },
    )?;

    Ok(ExtendedOverlayArtifact {
        plan: ExtendedOverlayPlan {
            chain,
            output_file_name,
            original_entry_count: layout.entry_count,
            append_start_entry_count,
            reserved_entry_count,
            final_entry_count,
            appended_tags,
        },
        bytes: artifact,
    })
}

fn validate_payloads(
    replacements: &[ReplacementSpec],
    new_tags: &[NewTagSpec],
) -> AuthoringResult<()> {
    if replacements.iter().any(|spec| spec.payload.is_empty())
        || new_tags.iter().any(|spec| spec.payload.is_empty())
    {
        return Err(AuthoringError::InvalidInput(
            "Extended-overlay payloads cannot be empty".into(),
        ));
    }
    if replacements
        .iter()
        .any(|spec| spec.payload.len() > u32::MAX as usize)
        || new_tags
            .iter()
            .any(|spec| spec.payload.len() > u32::MAX as usize)
    {
        return Err(AuthoringError::InvalidInput(
            "Extended-overlay payloads must fit the package entry-size field".into(),
        ));
    }
    Ok(())
}

/// Marks every historical gap row as a zero-length, classless package tombstone.
fn write_reserved_entry_rows(
    bytes: &mut [u8],
    entry_table_offset: usize,
    current_entry_count: usize,
    append_start_entry_count: usize,
) -> AuthoringResult<()> {
    for reserved_index in current_entry_count..append_start_entry_count {
        let row = reserved_index
            .checked_mul(ENTRY_HEADER_SIZE)
            .and_then(|size| entry_table_offset.checked_add(size))
            .ok_or_else(|| invalid("Reserved entry row offset overflowed"))?;
        let entry = bytes
            .get_mut(row..row + ENTRY_HEADER_SIZE)
            .ok_or_else(|| invalid("Reserved entry row extends beyond the package"))?;
        entry.fill(0);
        entry[..size_of::<u32>()].copy_from_slice(&u32::MAX.to_le_bytes());
    }
    Ok(())
}

fn validate_replacements(
    package_id: u16,
    entry_count: usize,
    replacements: &[ReplacementSpec],
) -> AuthoringResult<BTreeSet<usize>> {
    let mut indices = BTreeSet::new();
    for spec in replacements {
        let index = spec.tag.entry_index() as usize;
        if spec.tag.pkg_id() != package_id || index >= entry_count {
            return Err(AuthoringError::InvalidInput(format!(
                "Replacement tag {} is outside package {package_id:04x}",
                spec.tag
            )));
        }
        if !indices.insert(index) {
            return Err(AuthoringError::InvalidInput(format!(
                "Replacement tag {} was specified more than once",
                spec.tag
            )));
        }
    }
    Ok(indices)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct AppendedEntryMetadata {
    prefix: [u8; 8],
    reference: u32,
    file_type: u8,
    file_subtype: u8,
}

fn resolve_appended_metadata(
    package_directory: &Path,
    package_id: u16,
    original_entry_count: usize,
    new_tags: &[NewTagSpec],
    references: &[NewTagReference],
) -> AuthoringResult<Vec<AppendedEntryMetadata>> {
    // Keep validated entry tables local to one emission. Different templates
    // often live in the same package and should share its read-only inspection.
    let mut packages = BTreeMap::<u16, Vec<[u8; 8]>>::new();
    new_tags
        .iter()
        .zip(references.iter().copied())
        .map(|(spec, reference)| {
            let package = spec.template_tag.pkg_id();
            if let std::collections::btree_map::Entry::Vacant(entry) = packages.entry(package) {
                entry.insert(template_entry_prefixes(package_directory, package)?);
            }
            let mut prefix = *packages[&package]
                .get(spec.template_tag.entry_index() as usize)
                .ok_or_else(|| {
                    invalid(format!(
                        "Template tag {} is outside its package entry table",
                        spec.template_tag
                    ))
                })?;
            let template_reference = u32::from_le_bytes(
                prefix[..4]
                    .try_into()
                    .expect("an entry reference has exactly four bytes"),
            );
            let resolved_reference = resolve_new_tag_reference(
                reference,
                template_reference,
                package_id,
                original_entry_count,
                new_tags.len(),
            )?;
            prefix[..4].copy_from_slice(&resolved_reference.to_le_bytes());
            let template_type_info = u32::from_le_bytes(
                prefix[4..]
                    .try_into()
                    .expect("entry type metadata has exactly four bytes"),
            );
            let type_info = resolve_storage_mode(spec.storage, template_type_info)?;
            prefix[4..].copy_from_slice(&type_info.to_le_bytes());
            Ok(AppendedEntryMetadata {
                prefix,
                reference: resolved_reference,
                file_type: ((type_info >> 9) as u8) & 0x7F,
                file_subtype: ((type_info >> 6) as u8) & 0x07,
            })
        })
        .collect()
}

fn resolve_shared_tag_enrollments(
    package_id: u16,
    original_entry_count: usize,
    new_tags: &[NewTagSpec],
    metadata: &[AppendedEntryMetadata],
) -> AuthoringResult<Vec<(u32, u32)>> {
    if new_tags.len() != metadata.len() {
        return Err(validation(
            "Appended payload and entry-metadata counts disagree",
        ));
    }

    let appended_end = original_entry_count
        .checked_add(metadata.len())
        .ok_or_else(|| invalid("Appended tag range overflowed"))?;
    let mut companions_by_owner = BTreeMap::<u32, u32>::new();
    for (ordinal, companion) in metadata.iter().enumerate() {
        if companion.file_type != SHARED_TAG_COMPANION_FILE_TYPE
            || companion.file_subtype != SHARED_TAG_FILE_SUBTYPE
            || companion.reference != SHARED_TAG_COMPANION_CLASS
        {
            continue;
        }
        let companion_tag = appended_tag(package_id, original_entry_count, ordinal)?;
        let companion_payload = &new_tags[ordinal].payload;
        let payload_self = TagHash(read_u32(companion_payload, 0x08)?);
        let owner_tag = TagHash(read_u32(companion_payload, 0x0C)?);
        let owner_index = owner_tag.entry_index() as usize;
        if payload_self != companion_tag
            || !is_valid_package_tag(owner_tag)
            || owner_tag.pkg_id() != package_id
            || owner_index < original_entry_count
            || owner_index >= appended_end
        {
            return Err(AuthoringError::InvalidInput(format!(
                "Appended shared-tag companion {companion_tag} does not identify itself and an appended local owner"
            )));
        }
        let owner_ordinal = owner_index - original_entry_count;
        let owner = &metadata[owner_ordinal];
        if owner.file_type != SHARED_TAG_OWNER_FILE_TYPE
            || owner.file_subtype != SHARED_TAG_FILE_SUBTYPE
        {
            return Err(AuthoringError::InvalidInput(format!(
                "Appended shared-tag companion {companion_tag} names {owner_tag}, which is not a type-16.0 owner"
            )));
        }
        if companions_by_owner
            .insert(u32::from(owner_tag), u32::from(companion_tag))
            .is_some()
        {
            return Err(AuthoringError::InvalidInput(format!(
                "Appended type-16 owner {owner_tag} has more than one shared-tag companion"
            )));
        }
    }

    let mut enrollments = Vec::new();
    for (ordinal, owner) in metadata.iter().enumerate() {
        if owner.file_type != SHARED_TAG_OWNER_FILE_TYPE {
            continue;
        }
        let owner_tag = appended_tag(package_id, original_entry_count, ordinal)?;
        if owner.file_subtype != SHARED_TAG_FILE_SUBTYPE {
            return Err(AuthoringError::InvalidInput(format!(
                "Appended type-16 tag {owner_tag} uses unsupported subtype {}",
                owner.file_subtype
            )));
        }
        let companion = companions_by_owner
            .remove(&u32::from(owner_tag))
            .ok_or_else(|| {
                AuthoringError::InvalidInput(format!(
                    "Appended type-16 tag {owner_tag} is missing its shared-tag companion"
                ))
            })?;
        enrollments.push((u32::from(owner_tag), companion));
    }
    if !companions_by_owner.is_empty() {
        return Err(validation(
            "Appended shared-tag companion mapping did not converge",
        ));
    }
    Ok(enrollments)
}

fn appended_tag(
    package_id: u16,
    original_entry_count: usize,
    ordinal: usize,
) -> AuthoringResult<TagHash> {
    let index = original_entry_count
        .checked_add(ordinal)
        .ok_or_else(|| invalid("Appended tag index overflowed"))?;
    let index = u16::try_from(index)
        .map_err(|_| invalid("Appended tag index exceeds the package tag encoding"))?;
    Ok(TagHash::new(package_id, index))
}

fn validate_shared_tag_enrollments(
    source_bytes: &[u8],
    source_layout: &PackageLayout,
    candidate_bytes: &[u8],
    candidate_layout: &PackageLayout,
    appended: &[(u32, u32)],
) -> AuthoringResult<()> {
    let source_rows = source_layout.shared_tag_enrollment_rows(source_bytes)?;
    let candidate_rows = candidate_layout.shared_tag_enrollment_rows(candidate_bytes)?;
    let expected_count = source_layout
        .shared_tag_enrollment_count()
        .checked_add(appended.len())
        .ok_or_else(|| validation("Candidate shared-tag count overflowed"))?;
    if candidate_layout.shared_tag_enrollment_count() != expected_count {
        return Err(validation(format!(
            "Candidate shared-tag table has {} rows; expected {expected_count}",
            candidate_layout.shared_tag_enrollment_count()
        )));
    }
    if !candidate_rows.starts_with(source_rows) {
        return Err(validation(
            "Candidate changed existing shared-tag enrollment rows",
        ));
    }
    let suffix_capacity = appended
        .len()
        .checked_mul(8)
        .ok_or_else(|| validation("Shared-tag enrollment suffix size overflowed"))?;
    let mut expected_suffix = Vec::with_capacity(suffix_capacity);
    for (owner, companion) in appended {
        expected_suffix.extend_from_slice(&owner.to_le_bytes());
        expected_suffix.extend_from_slice(&companion.to_le_bytes());
    }
    if candidate_rows.get(source_rows.len()..) != Some(expected_suffix.as_slice()) {
        return Err(validation(
            "Candidate shared-tag enrollment suffix is incorrect",
        ));
    }
    Ok(())
}

fn resolve_storage_mode(mode: NewTagStorageMode, template_type_info: u32) -> AuthoringResult<u32> {
    match mode {
        NewTagStorageMode::InheritTemplate => Ok(template_type_info),
    }
}

fn resolve_reference_modes(
    new_tag_count: usize,
    reference_overrides: &[NewTagReferenceOverride],
) -> AuthoringResult<Vec<NewTagReference>> {
    let mut references = vec![NewTagReference::Template; new_tag_count];
    let mut overridden = BTreeSet::new();
    for entry_override in reference_overrides {
        if entry_override.new_tag_ordinal >= new_tag_count {
            return Err(AuthoringError::InvalidInput(format!(
                "Appended-tag reference override ordinal {} is outside the {} new tags",
                entry_override.new_tag_ordinal, new_tag_count
            )));
        }
        if !overridden.insert(entry_override.new_tag_ordinal) {
            return Err(AuthoringError::InvalidInput(format!(
                "Appended-tag reference override ordinal {} was specified more than once",
                entry_override.new_tag_ordinal
            )));
        }
        references[entry_override.new_tag_ordinal] = entry_override.reference;
    }
    if let Some(ordinal) = references.iter().find_map(|reference| match reference {
        NewTagReference::Appended(ordinal) if *ordinal >= new_tag_count => Some(*ordinal),
        _ => None,
    }) {
        return Err(AuthoringError::InvalidInput(format!(
            "Appended-tag reference ordinal {ordinal} is outside the {new_tag_count} new tags"
        )));
    }
    Ok(references)
}

fn resolve_new_tag_reference(
    reference: NewTagReference,
    template_reference: u32,
    package_id: u16,
    original_entry_count: usize,
    new_tag_count: usize,
) -> AuthoringResult<u32> {
    match reference {
        NewTagReference::Template => Ok(template_reference),
        NewTagReference::Appended(ordinal) => {
            if ordinal >= new_tag_count {
                return Err(AuthoringError::InvalidInput(format!(
                    "Appended-tag reference ordinal {ordinal} is outside the {new_tag_count} new tags"
                )));
            }
            let entry_index = original_entry_count
                .checked_add(ordinal)
                .ok_or_else(|| invalid("Appended-tag reference index overflowed"))?;
            let entry_index = u16::try_from(entry_index)
                .map_err(|_| invalid("Appended-tag reference index exceeds 16 bits"))?;
            Ok(u32::from(TagHash::new(package_id, entry_index)))
        }
    }
}

#[cfg(test)]
fn template_entry_prefix(
    package_directory: &Path,
    template_tag: TagHash,
) -> AuthoringResult<[u8; 8]> {
    template_entry_prefixes(package_directory, template_tag.pkg_id())?
        .get(template_tag.entry_index() as usize)
        .copied()
        .ok_or_else(|| {
            invalid(format!(
                "Template tag {template_tag} is outside its package entry table"
            ))
        })
}

fn template_entry_prefixes(
    package_directory: &Path,
    package_id: u16,
) -> AuthoringResult<Vec<[u8; 8]>> {
    let chain = discover_patch_chain(package_directory, package_id)?;
    let path = &chain.latest().path;
    let source = PackageD2PreBL::open(
        path.to_str()
            .ok_or_else(|| invalid("Template package path is not Unicode"))?,
    )
    .map_err(|error| {
        invalid(format!(
            "Could not read template package {}: {error}",
            path.display()
        ))
    })?;
    // This package is read-only: native stock layouts may have spare shared-tag capacity
    // (01dc has 2 live rows in an 8-row allocation). Do not require a donor to satisfy
    // PackageLayout's stricter rewrite contract just to copy its eight-byte entry prefix.
    if source.header.pkg_id != package_id
        || source.header.patch_id != u16::from(chain.latest().patch)
        || source.entries().len() != chain.latest().entry_count
    {
        return Err(invalid(
            "Template package identity disagrees with its discovered chain",
        ));
    }
    let offset = u64::from(source.header.entry_table_offset);
    let length = source
        .entries()
        .len()
        .checked_mul(ENTRY_HEADER_SIZE)
        .ok_or_else(|| invalid("Template entry table size overflow"))?;
    let mut file =
        File::open(path).map_err(|error| AuthoringError::io("open template entry", path, error))?;
    let size = file
        .metadata()
        .map_err(|error| AuthoringError::io("inspect template entry", path, error))?
        .len();
    if size != u64::from(source.header.file_size)
        || offset
            .checked_add(length as u64)
            .is_none_or(|end| end > size)
    {
        return Err(invalid(
            "Template entry is outside its declared package file",
        ));
    }
    file.seek(SeekFrom::Start(offset))
        .map_err(|error| AuthoringError::io("seek template entry", path, error))?;
    let mut table = vec![0; length];
    file.read_exact(&mut table)
        .map_err(|error| AuthoringError::io("read template entry", path, error))?;
    table
        .chunks_exact(ENTRY_HEADER_SIZE)
        .zip(source.entries())
        .map(|(row, entry)| {
            let prefix: [u8; 8] = row[..8].try_into().unwrap();
            let reference = u32::from_le_bytes(prefix[..4].try_into().unwrap());
            let type_info = u32::from_le_bytes(prefix[4..].try_into().unwrap());
            if reference != entry.reference
                || ((type_info >> 9) & 0x7F) as u8 != entry.file_type
                || ((type_info >> 6) & 0x07) as u8 != entry.file_subtype
            {
                return Err(invalid(
                    "Raw template prefix disagrees with the native package reader",
                ));
            }
            Ok(prefix)
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn write_payload(
    artifact: &mut Vec<u8>,
    layout: &PackageLayout,
    block_table_offset: usize,
    entry_index: usize,
    payload: &[u8],
    patch_id: u8,
    next_block: &mut usize,
    expected_blocks: usize,
    block_encoder: &PackageBlockEncoder,
) -> AuthoringResult<()> {
    let starting_block = *next_block;
    for chunk in payload.chunks(BLOCK_SIZE) {
        let encoded = block_encoder.encode(layout.package_id, chunk)?;
        let payload_offset = append_aligned(artifact, &encoded.stored);
        let block_row = block_table_offset + *next_block * BLOCK_HEADER_SIZE;
        write_block_header(artifact, block_row, payload_offset, &encoded, patch_id)?;
        *next_block += 1;
    }
    if *next_block - starting_block != expected_blocks {
        return Err(invalid("Payload block allocation disagreed with its plan"));
    }
    let entry_row = layout.entry_table_offset + entry_index * ENTRY_HEADER_SIZE;
    write_entry_location(artifact, entry_row, starting_block, 0, payload.len())
}

fn write_entry_location(
    bytes: &mut [u8],
    row: usize,
    starting_block: usize,
    starting_offset: usize,
    file_size: usize,
) -> AuthoringResult<()> {
    if starting_block >= MAX_BLOCK_COUNT
        || starting_offset >= BLOCK_SIZE
        || starting_offset % 16 != 0
    {
        return Err(invalid("Entry block location exceeds its encoded field"));
    }
    let file_size = u64::try_from(file_size).map_err(|_| invalid("Entry size is too large"))?;
    let block_info =
        (starting_block as u64) | (((starting_offset / 16) as u64) << 14) | (file_size << 28);
    write_u64(bytes, row + 8, block_info)
}

fn write_block_header(
    bytes: &mut [u8],
    row: usize,
    payload_offset: usize,
    payload: &EncodedPackageBlock,
    patch_id: u8,
) -> AuthoringResult<()> {
    write_u32(
        bytes,
        row,
        u32::try_from(payload_offset).map_err(|_| invalid("Block offset exceeds 32 bits"))?,
    )?;
    write_u32(
        bytes,
        row + 4,
        u32::try_from(payload.stored.len()).map_err(|_| invalid("Block size exceeds 32 bits"))?,
    )?;
    write_u16(bytes, row + 8, u16::from(patch_id))?;
    write_u16(bytes, row + 10, payload.flags)?;
    bytes[row + 12..row + 32].copy_from_slice(&Sha1::digest(&payload.stored));
    bytes[row + 32..row + BLOCK_HEADER_SIZE].copy_from_slice(&payload.gcm_tag);
    Ok(())
}

struct CandidateExpectations<'a> {
    replacements: &'a [ReplacementSpec],
    new_specs: &'a [NewTagSpec],
    appended: &'a [AppendedTag],
    appended_metadata: &'a [AppendedEntryMetadata],
    replacement_indices: &'a BTreeSet<usize>,
    append_start_entry_count: usize,
    reserved_entry_count: usize,
}

fn validate_candidate(
    source: &dyn Package,
    candidate: &dyn Package,
    expectations: CandidateExpectations<'_>,
) -> AuthoringResult<()> {
    let CandidateExpectations {
        replacements,
        new_specs,
        appended,
        appended_metadata,
        replacement_indices,
        append_start_entry_count,
        reserved_entry_count,
    } = expectations;
    if append_start_entry_count
        .checked_sub(source.entries().len())
        .is_none_or(|reserved| reserved != reserved_entry_count)
        || candidate.entries().len() != append_start_entry_count + appended.len()
    {
        return Err(validation("Candidate entry count is incorrect"));
    }
    for (index, (before, after)) in source.entries().iter().zip(candidate.entries()).enumerate() {
        let metadata_equal = before.reference == after.reference
            && before.file_type == after.file_type
            && before.file_subtype == after.file_subtype;
        let location_equal = before.starting_block == after.starting_block
            && before.starting_block_offset == after.starting_block_offset
            && before.file_size == after.file_size;
        if !metadata_equal || (!replacement_indices.contains(&index) && !location_equal) {
            return Err(validation(format!(
                "Candidate unexpectedly changed original entry {index}"
            )));
        }
    }
    for index in source.entries().len()..append_start_entry_count {
        let entry = candidate
            .entries()
            .get(index)
            .ok_or_else(|| validation(format!("Reserved entry {index} is missing")))?;
        if entry.reference != u32::MAX
            || entry.file_type != 0
            || entry.file_subtype != 0
            || entry.starting_block != 0
            || entry.starting_block_offset != 0
            || entry.file_size != 0
        {
            return Err(validation(format!(
                "Reserved entry {index} is not an inert package tombstone"
            )));
        }
    }
    for spec in replacements {
        let bytes = candidate.read_tag(spec.tag).map_err(|error| {
            validation(format!("Could not read replacement {}: {error}", spec.tag))
        })?;
        if bytes != spec.payload {
            return Err(validation(format!(
                "Replacement {} did not round-trip",
                spec.tag
            )));
        }
    }
    for ((spec, appended), metadata) in new_specs.iter().zip(appended).zip(appended_metadata) {
        let candidate_entry = candidate
            .entries()
            .get(appended.tag.entry_index() as usize)
            .ok_or_else(|| validation(format!("Appended tag {} has no entry", appended.tag)))?;
        if candidate_entry.reference != metadata.reference
            || candidate_entry.file_type != metadata.file_type
            || candidate_entry.file_subtype != metadata.file_subtype
            || candidate_entry.file_size as usize != spec.payload.len()
        {
            return Err(validation(format!(
                "Appended tag {} has incorrect reference, type metadata, or size",
                appended.tag
            )));
        }
        let bytes = candidate.read_tag(appended.tag).map_err(|error| {
            validation(format!(
                "Could not read appended tag {}: {error}",
                appended.tag
            ))
        })?;
        if bytes != spec.payload {
            return Err(validation(format!(
                "Appended tag {} did not round-trip",
                appended.tag
            )));
        }
    }
    let source_hashes = source.hash64_table();
    let candidate_hashes = candidate.hash64_table();
    let hashes_equal = source_hashes.len() == candidate_hashes.len()
        && source_hashes
            .iter()
            .zip(candidate_hashes)
            .all(|(left, right)| {
                left.hash64 == right.hash64
                    && left.hash32 == right.hash32
                    && left.reference == right.reference
            });
    if !hashes_equal {
        return Err(validation("Candidate changed the hash64 table"));
    }
    let source_named = source.named_tags();
    let candidate_named = candidate.named_tags();
    let named_equal = source_named.len() == candidate_named.len()
        && source_named
            .iter()
            .zip(candidate_named)
            .all(|(left, right)| {
                left.hash == right.hash
                    && left.class_hash == right.class_hash
                    && left.name == right.name
            });
    if !named_equal {
        return Err(validation("Candidate changed the named-tag table"));
    }
    validate_all_unchanged_entry_payloads(
        source.entries().len(),
        replacement_indices,
        |index| {
            source.read_entry(index).map_err(|error| {
                validation(format!("Could not read source entry {index}: {error}"))
            })
        },
        |index| {
            candidate.read_entry(index).map_err(|error| {
                validation(format!("Could not read candidate entry {index}: {error}"))
            })
        },
    )?;
    Ok(())
}

fn validate_all_unchanged_entry_payloads(
    original_entry_count: usize,
    replacement_indices: &BTreeSet<usize>,
    mut read_source: impl FnMut(usize) -> AuthoringResult<Vec<u8>>,
    mut read_candidate: impl FnMut(usize) -> AuthoringResult<Vec<u8>>,
) -> AuthoringResult<()> {
    for index in 0..original_entry_count {
        if replacement_indices.contains(&index) {
            continue;
        }
        if read_source(index)? != read_candidate(index)? {
            return Err(validation(format!(
                "Candidate changed original entry payload {index}"
            )));
        }
    }
    Ok(())
}

fn read_u32(bytes: &[u8], offset: usize) -> AuthoringResult<u32> {
    crate::tag_payload::read_u32(bytes, offset)
        .map_err(|_| invalid("32-bit read extends beyond payload"))
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) -> AuthoringResult<()> {
    crate::tag_payload::write_u16(bytes, offset, value)
        .map_err(|_| invalid("16-bit write extends beyond package"))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) -> AuthoringResult<()> {
    crate::tag_payload::write_u32(bytes, offset, value)
        .map_err(|_| invalid("32-bit write extends beyond package"))
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) -> AuthoringResult<()> {
    crate::tag_payload::write_u64(bytes, offset, value)
        .map_err(|_| invalid("64-bit write extends beyond package"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn invalid_field_offsets_leave_package_bytes_unchanged() {
        for offset in [7, 9, usize::MAX - 1, usize::MAX] {
            let mut bytes = [42; 8];
            assert!(super::read_u32(&bytes, offset).is_err());
            assert!(super::write_u16(&mut bytes, offset, 0).is_err());
            assert!(super::write_u32(&mut bytes, offset, 0).is_err());
            assert!(super::write_u64(&mut bytes, offset, 0).is_err());
            assert_eq!(bytes, [42; 8]);
        }
    }

    #[test]
    #[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES; read-only donor layout regression"]
    fn real_spare_shared_tag_capacity_is_valid_for_read_only_templates() {
        let packages = std::path::PathBuf::from(
            std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap(),
        );
        let chain = super::discover_patch_chain(&packages, 0x01DC).unwrap();
        let path = &chain.latest().path;
        let native = super::PackageD2PreBL::open(path.to_str().unwrap()).unwrap();
        let before = std::fs::metadata(path).unwrap().modified().unwrap();
        for index in [0, native.entries().len() - 1] {
            let prefix = super::template_entry_prefix(
                &packages,
                tiger_pkg::TagHash::new(0x01DC, index as u16),
            )
            .unwrap();
            assert_eq!(
                u32::from_le_bytes(prefix[..4].try_into().unwrap()),
                native.entries()[index].reference
            );
        }
        if native.entries().len() < 0x2000 {
            assert!(
                super::template_entry_prefix(
                    &packages,
                    tiger_pkg::TagHash::new(0x01DC, native.entries().len() as u16)
                )
                .is_err()
            );
        }
        assert_eq!(std::fs::metadata(path).unwrap().modified().unwrap(), before);
    }
    use super::*;

    #[test]
    fn retired_entry_gap_is_emitted_as_inert_tombstones() {
        let table_offset = 16;
        let mut bytes = vec![0xA5; table_offset + 4 * ENTRY_HEADER_SIZE];

        write_reserved_entry_rows(&mut bytes, table_offset, 1, 3)
            .expect("the reserved rows should fit");

        assert!(
            bytes[table_offset..table_offset + ENTRY_HEADER_SIZE]
                .iter()
                .all(|byte| *byte == 0xA5)
        );
        for index in 1..3 {
            let row = table_offset + index * ENTRY_HEADER_SIZE;
            assert_eq!(&bytes[row..row + 4], &u32::MAX.to_le_bytes());
            assert!(
                bytes[row + 4..row + ENTRY_HEADER_SIZE]
                    .iter()
                    .all(|byte| *byte == 0)
            );
        }
        assert!(
            bytes[table_offset + 3 * ENTRY_HEADER_SIZE..]
                .iter()
                .all(|byte| *byte == 0xA5)
        );
    }

    #[test]
    fn standalone_authoring_rejects_tracked_package_ids_before_package_discovery() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let error = build_standalone_package_with_references(
            directory.path(),
            0x058C,
            "w64_parhelion_assets_058c_0.pkg",
            &[NewTagSpec {
                template_tag: TagHash(0x8132_5796),
                payload: vec![1],
                storage: NewTagStorageMode::InheritTemplate,
            }],
            &[],
        )
        .expect_err("tracked package ids must not be used for standalone authored packages");

        assert!(
            error
                .to_string()
                .contains("outside the untracked AA0..=CFF window")
        );
    }

    #[test]
    fn resolves_forward_and_backward_appended_references() {
        let package_id = 0x058C;
        let original_entry_count = 100;
        let forward = resolve_new_tag_reference(
            NewTagReference::Appended(1),
            0xDEAD_BEEF,
            package_id,
            original_entry_count,
            3,
        )
        .expect("forward reference should resolve");
        let backward = resolve_new_tag_reference(
            NewTagReference::Appended(0),
            0xDEAD_BEEF,
            package_id,
            original_entry_count,
            3,
        )
        .expect("backward reference should resolve");
        assert_eq!(forward, u32::from(TagHash::new(package_id, 101)));
        assert_eq!(backward, u32::from(TagHash::new(package_id, 100)));
    }

    #[test]
    fn template_reference_mode_is_byte_compatible() {
        let template_reference = 0x8131_933F;
        assert_eq!(
            resolve_new_tag_reference(
                NewTagReference::Template,
                template_reference,
                0x058C,
                100,
                1
            )
            .expect("template reference should resolve"),
            template_reference
        );
        assert_eq!(
            resolve_reference_modes(2, &[]).expect("empty overrides should resolve"),
            vec![NewTagReference::Template, NewTagReference::Template]
        );
    }

    #[test]
    fn storage_mode_preserves_the_template_selector() {
        let type16 = 0x0000_2019;
        let resolved = resolve_storage_mode(NewTagStorageMode::InheritTemplate, type16)
            .expect("the template selector should be preserved");

        assert_eq!(resolved, type16);
    }

    #[test]
    fn derives_shared_tag_enrollment_from_companion_payload_identity() {
        let package_id = 0x0914;
        let original_entry_count = 0x055F;
        let owner = appended_tag(package_id, original_entry_count, 0).unwrap();
        let companion = appended_tag(package_id, original_entry_count, 2).unwrap();
        let mut companion_payload = vec![0u8; 0x10];
        companion_payload[0x08..0x0C].copy_from_slice(&u32::from(companion).to_le_bytes());
        companion_payload[0x0C..0x10].copy_from_slice(&u32::from(owner).to_le_bytes());
        let specs = [
            NewTagSpec {
                template_tag: TagHash(0x8132_5796),
                payload: vec![1],
                storage: NewTagStorageMode::InheritTemplate,
            },
            NewTagSpec {
                template_tag: TagHash(0x8132_5797),
                payload: vec![2],
                storage: NewTagStorageMode::InheritTemplate,
            },
            NewTagSpec {
                template_tag: TagHash(0x8132_5798),
                payload: companion_payload,
                storage: NewTagStorageMode::InheritTemplate,
            },
        ];
        let metadata = [
            AppendedEntryMetadata {
                prefix: [0; 8],
                reference: 0x8080_4A53,
                file_type: SHARED_TAG_OWNER_FILE_TYPE,
                file_subtype: SHARED_TAG_FILE_SUBTYPE,
            },
            AppendedEntryMetadata {
                prefix: [0; 8],
                reference: 0x8080_4A69,
                file_type: 0x08,
                file_subtype: 0,
            },
            AppendedEntryMetadata {
                prefix: [0; 8],
                reference: SHARED_TAG_COMPANION_CLASS,
                file_type: SHARED_TAG_COMPANION_FILE_TYPE,
                file_subtype: SHARED_TAG_FILE_SUBTYPE,
            },
        ];

        assert_eq!(
            resolve_shared_tag_enrollments(package_id, original_entry_count, &specs, &metadata)
                .unwrap(),
            vec![(u32::from(owner), u32::from(companion))]
        );

        let mut malformed = metadata;
        malformed[2].reference = 0x8080_4A53;
        assert!(
            resolve_shared_tag_enrollments(package_id, original_entry_count, &specs, &malformed)
                .expect_err("a type-16 owner without its companion must fail")
                .to_string()
                .contains("missing its shared-tag companion")
        );
        assert!(
            resolve_shared_tag_enrollments(
                package_id,
                original_entry_count + 2,
                &specs[2..],
                &metadata[2..]
            )
            .expect_err("an orphan shared-tag companion must fail")
            .to_string()
            .contains("appended local owner")
        );
    }

    #[test]
    fn rejects_invalid_and_duplicate_reference_ordinals() {
        let invalid_target =
            resolve_new_tag_reference(NewTagReference::Appended(2), 0, 0x058C, 100, 2)
                .expect_err("out-of-range appended target must fail");
        assert!(
            invalid_target
                .to_string()
                .contains("outside the 2 new tags")
        );

        let invalid_override = resolve_reference_modes(
            2,
            &[NewTagReferenceOverride {
                new_tag_ordinal: 2,
                reference: NewTagReference::Template,
            }],
        )
        .expect_err("out-of-range override ordinal must fail");
        assert!(
            invalid_override
                .to_string()
                .contains("outside the 2 new tags")
        );

        let duplicate = resolve_reference_modes(
            2,
            &[
                NewTagReferenceOverride {
                    new_tag_ordinal: 1,
                    reference: NewTagReference::Appended(0),
                },
                NewTagReferenceOverride {
                    new_tag_ordinal: 1,
                    reference: NewTagReference::Template,
                },
            ],
        )
        .expect_err("duplicate override ordinal must fail");
        assert!(duplicate.to_string().contains("specified more than once"));
    }

    #[test]
    fn unchanged_payload_validation_checks_every_original_entry() {
        let source = (0u8..8).map(|value| vec![value]).collect::<Vec<_>>();
        let mut candidate = source.clone();
        candidate[6] = vec![0xFF];
        let error = validate_all_unchanged_entry_payloads(
            source.len(),
            &BTreeSet::new(),
            |index| Ok(source[index].clone()),
            |index| Ok(candidate[index].clone()),
        )
        .expect_err("a non-sampled original entry mutation must fail");
        assert!(error.to_string().contains("payload 6"));

        let replacements = BTreeSet::from([6]);
        validate_all_unchanged_entry_payloads(
            source.len(),
            &replacements,
            |index| Ok(source[index].clone()),
            |index| Ok(candidate[index].clone()),
        )
        .expect("declared replacement payloads are checked separately");
    }

    #[test]
    #[ignore = "requires SUNDIAL_TEST_PACKAGES and SUNDIAL_TEST_PACKAGE_ID"]
    fn real_overlay_round_trips_mutual_references() {
        let package_directory = PathBuf::from(
            std::env::var_os("SUNDIAL_TEST_PACKAGES")
                .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages"),
        );
        let package_id_text = std::env::var("SUNDIAL_TEST_PACKAGE_ID")
            .expect("SUNDIAL_TEST_PACKAGE_ID must be a hexadecimal package id");
        let package_id = u16::from_str_radix(package_id_text.trim_start_matches("0x"), 16)
            .expect("SUNDIAL_TEST_PACKAGE_ID should be hexadecimal");
        let chain = discover_patch_chain(&package_directory, package_id)
            .expect("real package chain should be discoverable");
        let source_path = &chain.latest().path;
        let source = PackageD2PreBL::open(
            source_path
                .to_str()
                .expect("real package path should be Unicode"),
        )
        .expect("real source package should open");
        let original_entry_count = source.entries().len();
        let ordinary_templates = source
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                entry.file_type != SHARED_TAG_OWNER_FILE_TYPE
                    && !(entry.file_type == SHARED_TAG_COMPANION_FILE_TYPE
                        && entry.file_subtype == SHARED_TAG_FILE_SUBTYPE
                        && entry.reference == SHARED_TAG_COMPANION_CLASS)
            })
            .map(|(index, _)| TagHash::new(package_id, index as u16))
            .take(2)
            .collect::<Vec<_>>();
        assert_eq!(
            ordinary_templates.len(),
            2,
            "test package needs two ordinary non-shared entries"
        );
        let template_a = ordinary_templates[0];
        let template_b = ordinary_templates[1];
        let new_tags = [
            NewTagSpec {
                template_tag: template_a,
                payload: b"mutual-data".to_vec(),
                storage: NewTagStorageMode::InheritTemplate,
            },
            NewTagSpec {
                template_tag: template_b,
                payload: b"mutual-header".to_vec(),
                storage: NewTagStorageMode::InheritTemplate,
            },
        ];
        let artifact = build_extended_overlay_with_references(
            &package_directory,
            package_id,
            &[],
            &new_tags,
            &[
                NewTagReferenceOverride {
                    new_tag_ordinal: 0,
                    reference: NewTagReference::Appended(1),
                },
                NewTagReferenceOverride {
                    new_tag_ordinal: 1,
                    reference: NewTagReference::Appended(0),
                },
            ],
        )
        .expect("real mutual-reference overlay should build and validate");
        let candidate_path = package_directory.join(&artifact.plan.output_file_name);
        let candidate = PackageD2PreBL::from_reader(
            candidate_path
                .to_str()
                .expect("candidate path should be Unicode"),
            Cursor::new(artifact.bytes().to_vec()),
        )
        .expect("real candidate should reopen");
        let entries = candidate.entries();
        assert_eq!(
            entries[original_entry_count].reference,
            u32::from(TagHash::new(package_id, (original_entry_count + 1) as u16))
        );
        assert_eq!(
            entries[original_entry_count + 1].reference,
            u32::from(TagHash::new(package_id, original_entry_count as u16))
        );
        let baseline = build_extended_overlay(&package_directory, package_id, &[], &new_tags[..1])
            .expect("baseline template overlay should build");
        let overridden_template = build_extended_overlay_with_references(
            &package_directory,
            package_id,
            &[],
            &new_tags[..1],
            &[NewTagReferenceOverride {
                new_tag_ordinal: 0,
                reference: NewTagReference::Template,
            }],
        )
        .expect("template override should build");
        assert_eq!(baseline.bytes(), overridden_template.bytes());
    }
}
