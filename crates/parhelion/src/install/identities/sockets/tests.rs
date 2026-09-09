use super::*;
use crate::tag_payload::{write_relative_pointer, write_u16, write_u32, write_u64};

fn item(defaults: &[u16]) -> Vec<u8> {
    let resource = 0x100;
    let header = resource + 24;
    let rows = header + 16;
    let mut data = vec![0; rows + defaults.len() * ITEM_ORDINARY_SOCKET_ROW_SIZE];
    write_relative_pointer(&mut data, ITEM_ORDINARY_SOCKET_POINTER_OFFSET, resource).unwrap();
    write_u64(&mut data, resource, defaults.len() as u64).unwrap();
    write_relative_pointer(&mut data, resource + 8, header).unwrap();
    write_u64(&mut data, header, defaults.len() as u64).unwrap();
    write_u32(&mut data, header + 8, ITEM_ORDINARY_SOCKET_ROW_CLASS).unwrap();
    for (index, default) in defaults.iter().enumerate() {
        write_u16(
            &mut data,
            rows + index * ITEM_ORDINARY_SOCKET_ROW_SIZE + ITEM_ORDINARY_SOCKET_DEFAULT_PLUG_OFFSET,
            *default,
        )
        .unwrap();
    }
    data
}

#[test]
fn native_socket_defaults_follow_generation_indices_and_keep_empty_lanes() {
    let data = item(&[0, u16::MAX, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]);
    let old = decode_defaults(&data, &[100, 200]).unwrap();
    let new = decode_defaults(&data, &[300, 400]).unwrap();
    assert_eq!(old.len(), 12);
    assert_eq!(&old[..3], &[Some(100), None, Some(200)]);
    assert_eq!(&new[..3], &[Some(300), None, Some(400)]);
    assert!(decode_defaults(&[0; 0x100], &[]).unwrap().is_empty());
}

#[test]
fn malformed_native_layouts_and_unresolved_defaults_block_migration() {
    assert!(decode_defaults(&item(&[1]), &[100]).is_err());
    assert!(decode_defaults(&item(&[0]), &[u32::MAX]).is_err());
    assert!(decode_defaults(&item(&[0; 13]), &[100]).is_err());
    let mut truncated = item(&[0; 8]);
    truncated.pop();
    assert!(decode_defaults(&truncated, &[100]).is_err());
    let mut bad_class = item(&[0; 8]);
    write_u32(&mut bad_class, 0x120, 0).unwrap();
    assert!(decode_defaults(&bad_class, &[100]).is_err());
}

#[test]
#[ignore = "read-only native metadata check, requires PARHELION_LIFECYCLE_SOURCE_PACKAGES"]
fn installed_weapon_and_private_plug_socket_layouts_are_readable() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_LIFECYCLE_SOURCE_PACKAGES").unwrap());
    let (hashes, _) = installed_identities(&packages).unwrap();
    let defaults = generation_socket_defaults(&packages, &packages, &hashes).unwrap();
    assert_eq!(defaults.len(), hashes.len());
    let socket_items = defaults
        .values()
        .filter(|sockets| !sockets.is_empty())
        .count();
    assert!(socket_items > 0);
    println!(
        "Read native sockets for {} authored definitions, including {socket_items} with ordinary sockets",
        defaults.len()
    );
}
