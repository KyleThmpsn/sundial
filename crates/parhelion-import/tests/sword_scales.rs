//! Opt-in package verification for modern sword angular modifier values.

use std::{env, fs, path::PathBuf};

use parhelion_import::d2_mot::{gameplay::perks, payload::Payload, reader::Reader};
use serde_json::json;
use sha2::{Digest, Sha256};

fn assert_package_values(owner: &Payload) -> Result<(), Box<dyn std::error::Error>> {
    // Independently recorded offsets in the shipped source owner establish the
    // package oracle. The lowering API must locate the rows by checked schema.
    assert_eq!(owner.0.len(), 0x4A0);
    assert_eq!(owner.u32(0x374)?, 0x8080_2D32);
    assert_eq!(owner.u32(0x3E4)?, 0x8080_2D32);
    assert_eq!(owner.u8(0x370 + 20)?, 1);
    assert_eq!(owner.u8(0x3E0 + 20)?, 1);
    assert_eq!(owner.u8(0x370 + 96)?, 12);
    assert_eq!(owner.u8(0x3E0 + 96)?, 12);
    assert_eq!(owner.u16(0x370 + 90)?, 4);
    assert_eq!(owner.u16(0x3E0 + 90)?, 5);
    assert_eq!(owner.u32(0x370 + 16)?, 0x3F00_0000);
    assert_eq!(owner.u32(0x3E0 + 16)?, 0x3DF5_C28F);

    Ok(())
}

#[test]
#[ignore = "Requires PARHELION_IMPORT_MODERN_PACKAGES and PARHELION_IMPORT_PERK_OUTPUT"]
fn modern_sword_angular_scales_follow_package_rows() -> Result<(), Box<dyn std::error::Error>> {
    let packages = PathBuf::from(
        env::var_os("PARHELION_IMPORT_MODERN_PACKAGES")
            .ok_or_else(|| "Set PARHELION_IMPORT_MODERN_PACKAGES".to_owned())?,
    );
    let output = PathBuf::from(
        env::var_os("PARHELION_IMPORT_PERK_OUTPUT")
            .ok_or_else(|| "Set PARHELION_IMPORT_PERK_OUTPUT".to_owned())?,
    )
    .join("sword-scales");
    let mut source = Reader::new(&packages, &output.join("source"), true)?;
    let owner = source.tag(0x80C3_78E1, Some(0x8080_9B06))?;

    assert_package_values(&owner)?;

    let scales = perks::sword::angular_scales(&owner)?;
    assert_eq!(scales.near_bits, 0x3F00_0000);
    assert_eq!(scales.far_bits, 0x3DF5_C28F);

    let mut retuned = owner.0.clone();
    retuned[0x370 + 16..0x370 + 20].copy_from_slice(&0.25f32.to_bits().to_le_bytes());
    let retuned_scales = perks::sword::angular_scales(&Payload(retuned))?;
    assert_eq!(retuned_scales.near_bits, 0.25f32.to_bits());
    assert_eq!(retuned_scales.far_bits, scales.far_bits);

    let mut malformed = owner.0.clone();
    malformed[0x3E0 + 90..0x3E0 + 92].copy_from_slice(&4u16.to_le_bytes());
    assert!(perks::sword::angular_scales(&Payload(malformed)).is_err());

    let mut malformed = owner.0.clone();
    malformed[0x370 + 20] = 0;
    assert!(perks::sword::angular_scales(&Payload(malformed)).is_err());

    // Another shipped two-row owner uses the same input numbers and operation
    // on a different component. It must not be accepted as sword tracking.
    let other = source.tag(0x80C3_FDDE, Some(0x8080_9B06))?;
    assert_eq!(other.u8(0x370 + 96)?, 7);
    assert_eq!(other.u8(0x3E0 + 96)?, 7);
    assert!(perks::sword::angular_scales(&other).is_err());

    fs::create_dir_all(&output)?;
    fs::write(
        output.join("verified-angular-scales.json"),
        serde_json::to_vec_pretty(&json!({
            "source_owner": "80C378E1",
            "source_owner_class": "80809B06",
            "source_sha256": format!("{:x}", Sha256::digest(&owner.0)),
            "other_two_row_owner": "80C3FDDE",
            "other_owner_sha256": format!("{:x}", Sha256::digest(&other.0)),
            "source_component": 12,
            "source_near_input": 4,
            "source_far_input": 5,
            "source_operation": 1,
            "near_bits": format!("{:08X}", scales.near_bits),
            "far_bits": format!("{:08X}", scales.far_bits),
            "gameplay_verified": false
        }))?,
    )?;
    source.finish()?;
    println!("Verified sword angular scales at {}", output.display());
    Ok(())
}
