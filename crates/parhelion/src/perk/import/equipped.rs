//! Prepare a source-validated equip lifecycle using a privately cloned modifier graph.
use super::*;
use parhelion_import::d2_mot::gameplay::perks::equipped;
use sundial::package_authoring::sandbox_perk::program::{Asset, AttachmentTarget};

pub(super) fn prepare(
    request: &Request<'_>,
    manager: &PackageManager,
    reader: &mut Reader,
    output: &Path,
    donor: &WeaponDonor,
    choices: Vec<HexHash>,
) -> Result<Prepared, String> {
    let source = equipped::extract(reader, request.plug_hash).map_err(error)?;
    let presentation = presentation::read(reader, request.plug_hash).map_err(error)?;
    let mut native = modifier(manager, &source.owner, source.modifier_offset)?;
    // The stock envelope also contains a presentation owner. The source constant
    // modifier has no such component, so leave it out of this private copy.
    let graph = Payload(native.graph.clone());
    for row in graph.array(16, 12, Some(0x80809C04)).map_err(error)? {
        let tag = graph.u32(row).map_err(error)?;
        if tag != native.owner_tag {
            let owner = Payload(checked_tag(manager, tag, 0x80809C36)?);
            let settings = owner.pointer(24).map_err(error)?;
            if owner.u32(settings - 4).map_err(error)? != 0x80804205 {
                return Err(
                    "The native modifier envelope has an unsupported extra component".into(),
                );
            }
            native.patch.remove_owners.push(tag);
        }
    }
    let name = plug_name(request, &presentation);
    let program = Program {
        name: name.clone(),
        trigger: Trigger::Native,
        duration_ms: 0,
        cooldown_ms: 0,
        native_trigger: Some(node(source.activation)),
        native_removal: Some(node(source.removal)),
        actions: vec![Action::Attach {
            asset: Asset {
                graph: MODIFIER_GRAPH,
                ..Default::default()
            },
            mode: AttachmentTarget::ThisItem,
            keys: [0x811C9DC5; 2],
            float_bits: [0; 4],
        }],
        native_asset_patches: vec![native.patch],
        ..Program::default()
    };
    program.validate()?;
    let mut effect = super::super::PerkRecipe::effect(request.source_perk_index);
    effect.program = Some(program);
    reader.finish().map_err(error)?;
    let at = source.modifier_offset;
    let provenance = json!({
        "status":"experimental equipped modifier candidate", "gameplay_verified":false,
        "presentation":presentation.report,
        "source": {"plug_hash":request.plug_hash,"item_tag":source.assignment.item_tag,
            "action_tag":source.assignment.action_tag,"controller_sha256":digest(&source.assignment.controller.0),
            "entity":source.entity_tag,"owner":source.owner_tag,"path":source.path,
            "owner_sha256":digest(&source.owner.0),"modifier_offset":at,
            "amount":source.owner.f32(at + 16).map_err(error)?,
            "operation":source.owner.u8(at + 20).map_err(error)?,
            "component":source.owner.u8(at + 96).map_err(error)?,
            "input":source.owner.u16(at + 90).map_err(error)?},
        "native":{"template_graph":MODIFIER_GRAPH,"template_owner":native.owner_tag,
            "template_owner_sha256":digest(&native.owner),"activation":14,"removal":15,
            "attachment_target":0,"private_graph":true},
        "remaining":["In-game equip, repeated reload, perk selection and unequip cleanup", "Stock donor gameplay isolation"]
    });
    let plug = Plug {
        presentation: &presentation,
        name,
        sandbox_perks: vec![effect],
        projectile: None,
    };
    finish(request, output, donor, choices, plug, |_| {}, provenance)
}
