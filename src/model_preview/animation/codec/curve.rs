//! Adapt shared Shadowkeep curve tracks to the preview's local transforms.
use super::*;
use crate::animation::curve::{Channel as CurveChannel, Decoder};

pub(super) fn apply(
    bytes: &[u8],
    data: usize,
    tracks: &Tracks,
    poses: &mut [Vec<Transform>],
) -> Result<(), String> {
    let decoder = Decoder::read(bytes, data, tracks.0.each_ref().map(Vec::len), poses.len())?;
    for (channel, bones) in tracks.groups() {
        let kind = match channel {
            Channel::Scale => CurveChannel::Scale,
            Channel::Rotation => CurveChannel::Rotation,
            Channel::Position => CurveChannel::Position,
        };
        for (track, &bone) in bones.iter().enumerate() {
            for (pose, point) in poses.iter_mut().zip(decoder.samples(kind, track)?) {
                channel.store(&mut pose[bone], &point);
            }
        }
    }
    Ok(())
}
