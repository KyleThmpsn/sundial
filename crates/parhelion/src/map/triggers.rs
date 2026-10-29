//! Neutral mission volumes and their player sensors share one private object per area.
use std::collections::BTreeSet;

use parhelion_import::halo_reach::level::volumes::Volume;
use sundial::package_authoring::{PackageManager, fnv1_name_hash};
use tiger_pkg::TagHash;

use super::placed::{self, Placed};
use crate::{
    AuthoringResult, NewTagSpec, NewTagStorageMode,
    error::invalid,
    tag_payload::{
        append_native_array, array_at, bounded_relative_target, read_tag, read_u16, read_u32,
        read_u64, synchronize_payload_size, write_u16, write_u32, write_u64,
    },
};

const VOLUME_COMPONENT: u32 = 0x8080_99C8;
const SENSOR_COMPONENT: u32 = 0x8080_9522;
// These are donor roles, validated against the paired placed records and slot references.
const NEUTRAL_VOLUME_ENTITY: u32 = 0x80BF_DDCB;

#[derive(Clone)]
pub(super) struct Triggers {
    object: (TagHash, Vec<u8>),
    volume: Placed,
    sensor: Placed,
    packfile: (TagHash, Vec<u8>),
    dependencies: Vec<TagHash>,
    volumes: Vec<Volume>,
    key: u32,
}

impl Triggers {
    /// Finds a neutral volume record and a sensor naming its first volume in this object. The pair
    /// holds no volumes until [`Self::placed`] gives it an area's.
    pub(super) fn find(
        manager: &PackageManager,
        object: TagHash,
        payload: &[u8],
    ) -> AuthoringResult<Option<Self>> {
        if read_u64(payload, 0x38)? == 0 {
            return Ok(None);
        }
        let (count, _, groups, _) = array_at(payload, 0x38)?;
        if count != 2 {
            return Ok(None);
        }
        let mut records = Vec::new();
        for group in 0..count {
            let field = groups + group * 24 + 8;
            if read_u64(payload, field)? == 0 {
                continue;
            }
            let (handles, _, rows, _) = array_at(payload, field)?;
            for index in 0..handles {
                if let Some(record) = placed::read(manager, read_u32(payload, rows + index * 4)?)? {
                    records.push(record);
                }
            }
        }
        for volume in &records {
            let record = &volume.payloads[2];
            let Ok(descriptor) = placed::keyed(record) else {
                continue;
            };
            if read_u32(record, descriptor + 4)? != VOLUME_COMPONENT {
                continue;
            }
            let (count, _, rows, row_class) = array_at(record, descriptor + 0x38)?;
            if count == 0 || row_class != 0x8080_99D0 {
                continue;
            }
            if (0..count).any(|index| {
                read_u32(record, rows + index * 0x120 + 0x20).ok() != Some(NEUTRAL_VOLUME_ENTITY)
            }) {
                continue;
            }
            let reference = read_u64(record, rows + 0x0C)?;
            if (reference >> 32) as u16 != 60 || reference as u32 != read_u32(payload, 0x0C)? {
                return Err(invalid("Neutral trigger volume has a stale object slot"));
            }
            for sensor in &records {
                let sensor_record = &sensor.payloads[2];
                let Ok(primary) =
                    bounded_relative_target(sensor_record, 0x10, "Player sensor component")
                else {
                    continue;
                };
                if primary < 4 || read_u32(sensor_record, primary - 4)? != SENSOR_COMPONENT {
                    continue;
                }
                let secondary = placed::keyed(sensor_record)?;
                if read_u16(sensor_record, secondary + 0x34)? != 31 {
                    continue;
                }
                let (count, _, sensors, class) = array_at(sensor_record, secondary + 0x58)?;
                if count != 1
                    || class != 0x8080_9C42
                    || read_u64(sensor_record, sensors)? != reference
                {
                    continue;
                }
                let packfile = TagHash(read_u32(record, descriptor + 0x48)?);
                let mut dependencies = BTreeSet::new();
                for owner in [volume, sensor] {
                    let required = sundial::package_authoring::sandbox_perk::entity::residency::owner_requirements(
                        manager, owner.tags[2].0, &owner.payloads[2]).map_err(invalid)?;
                    dependencies.extend(required.into_iter().map(|requirement| requirement.tag));
                }
                dependencies.extend(
                    sundial::package_authoring::sandbox_perk::entity::residency::additions(
                        manager,
                        [NEUTRAL_VOLUME_ENTITY],
                    )
                    .map_err(invalid)?,
                );
                return Ok(Some(Self {
                    object: (object, payload.to_vec()),
                    volume: volume.clone(),
                    sensor: sensor.clone(),
                    packfile: (
                        packfile,
                        read_tag(manager, packfile, "Neutral trigger packfile")?,
                    ),
                    dependencies: dependencies.into_iter().map(TagHash).collect(),
                    volumes: Vec::new(),
                    key: 0,
                }));
            }
        }
        Ok(None)
    }

    /// The pair for the area `area`, holding its `volumes`.
    pub(super) fn placed(&self, volumes: &[Volume], area: &str) -> Self {
        Self {
            volumes: volumes.to_vec(),
            key: fnv1_name_hash(&format!("{area}/triggers")),
            ..self.clone()
        }
    }

    pub(super) fn bindings(&self) -> super::mission::Triggers {
        super::mission::Triggers {
            key: self.key,
            template: self.object.0,
            names: self
                .volumes
                .iter()
                .map(|volume| volume.name.clone())
                .collect(),
        }
    }

    pub(super) fn tags(
        &self,
        at: &dyn Fn(usize) -> AuthoringResult<TagHash>,
        next: &mut usize,
        rename: &dyn Fn(&mut Vec<u8>),
    ) -> AuthoringResult<(Vec<NewTagSpec>, TagHash, Vec<TagHash>)> {
        let n = self.volumes.len();
        let n16 = u16::try_from(n)
            .ok()
            .filter(|n| *n <= u16::MAX / 2)
            .ok_or_else(|| invalid("Too many mission triggers"))?;
        let mut names = BTreeSet::new();
        let mut hashes = BTreeSet::new();
        for volume in &self.volumes {
            for name in [volume.name.clone(), format!("pt_{}", volume.name)] {
                if !names.insert(name.clone()) || !hashes.insert(fnv1_name_hash(&name)) {
                    return Err(invalid("Mission trigger names alias another slot"));
                }
            }
        }
        let mut take = || {
            let tag = at(*next)?;
            *next += 1;
            Ok::<_, crate::AuthoringError>(tag)
        };
        let object = take()?;
        let volume_tags = [take()?, take()?, take()?];
        let packfile_tag = take()?;
        let mut added = self.dependencies.clone();
        added.extend([
            object,
            volume_tags[0],
            volume_tags[1],
            volume_tags[2],
            packfile_tag,
        ]);
        let spec = |template_tag, payload| NewTagSpec {
            template_tag,
            payload,
            storage: NewTagStorageMode::InheritTemplate,
        };
        let mut volume = placed::clone_placed(&self.volume, volume_tags[1], volume_tags[2])?;
        let (mut record, packfile) =
            super::volumes::named(&volume[2], &self.packfile.1, &self.volumes, self.key, n16)?;
        let descriptor = placed::keyed(&record)?;
        write_u32(&mut record, descriptor + 0x48, packfile_tag.0)?;
        let (_, _, rows, _) = array_at(&record, descriptor + 0x38)?;
        for index in 0..n {
            let shape =
                bounded_relative_target(&record, rows + index * 0x120 + 0x98, "Trigger shape")?;
            write_u32(&mut record, shape + 0x10, packfile_tag.0)?;
        }
        volume[2] = record;
        let mut content = volume
            .into_iter()
            .enumerate()
            .map(|(index, payload)| spec(self.volume.tags[index], payload))
            .collect::<Vec<_>>();
        content.push(spec(self.packfile.0, packfile));
        let mut handles = Vec::new();
        let mut slots = vec![0u8; n * 2 * 8];
        for (index, volume) in self.volumes.iter().enumerate() {
            let tags = [take()?, take()?, take()?];
            added.extend(tags);
            handles.push(tags[0]);
            let name = format!("pt_{}", volume.name);
            let mut sensor = placed::clone_placed(&self.sensor, tags[1], tags[2])?;
            placed::set_slot(&mut sensor[2], self.key, 31, index as u16)?;
            let descriptor = placed::keyed(&sensor[2])?;
            let (_, _, rows, _) = array_at(&sensor[2], descriptor + 0x58)?;
            write_u64(
                &mut sensor[2],
                rows,
                u64::from(self.key) | 60u64 << 32 | ((n + index) as u64) << 48,
            )?;
            sensor[2] = placed::renamed(&sensor[2], &name)?;
            content.extend(
                sensor
                    .into_iter()
                    .enumerate()
                    .map(|(i, payload)| spec(self.sensor.tags[i], payload)),
            );
            for (slot, kind, name) in [(index, 31, &name), (n + index, 60, &volume.name)] {
                write_u16(&mut slots, slot * 8, kind)?;
                write_u32(&mut slots, slot * 8 + 4, fnv1_name_hash(name))?;
            }
        }
        let mut payload = self.object.1.clone();
        rename(&mut payload);
        write_u32(&mut payload, 0x0C, self.key)?;
        write_u16(&mut payload, 0x18, n16)?;
        write_u16(&mut payload, 0x1A, n16)?;
        append_native_array(&mut payload, 0x20, 0x8080_9B3A, n * 2, &slots)?;
        let mut seen = BTreeSet::new();
        for group in 0..2 {
            let groups = array_at(&payload, 0x38)?.2;
            let bubble = read_u32(&payload, groups + group * 24)?;
            if !seen.insert(bubble) || ![u32::MAX, 0].contains(&bubble) {
                return Err(invalid("Trigger template has unexpected bubble groups"));
            }
            let list = if bubble == u32::MAX {
                &volume_tags[..1]
            } else {
                &handles
            };
            let rows = list
                .iter()
                .flat_map(|tag| tag.0.to_le_bytes())
                .collect::<Vec<_>>();
            append_native_array(
                &mut payload,
                groups + group * 24 + 8,
                0x8080_9466,
                list.len(),
                &rows,
            )?;
        }
        let mut specs = vec![spec(self.object.0, synchronize_payload_size(payload)?)];
        specs.extend(content);
        Ok((specs, object, added))
    }
}
