//! A private wheeled implementation using the registered Shadowkeep car callbacks.
use super::curve::Program;
use crate::{
    presentation::{Graph, append, put},
    tiger::{
        allocation::Row, entity::compiled::authored::Provider, payload::Payload, reader::Reader,
    },
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub(super) const OWNER: u32 = 0xfffffffe;
pub(super) const INSTANCE: usize = 0x80;
pub(super) const DEFINITION: usize = INSTANCE + 0x308;
const ENGINE: usize = DEFINITION + 0x3a8;
const SYMBOL: &str = "vehicle-motion";

pub(super) struct Callback {
    pub key: u32,
    pub offset: usize,
    pub condition: u16,
    pub metadata: Payload,
}

pub(super) struct Motion {
    pub payload: Payload,
    pub callbacks: Vec<Callback>,
    pub inputs: Vec<Input>,
    pub retain_hover_update: bool,
    pub report: Value,
}

pub(super) struct Input {
    pub offset: usize,
    pub method: u32,
}

impl Motion {
    pub fn providers(&self) -> Vec<Provider<'_>> {
        self.callbacks
            .iter()
            .map(|p| Provider {
                key: p.key,
                offset: p.offset,
                metadata: &p.metadata,
                condition: p.condition,
            })
            .collect()
    }
}

pub(super) struct Writer {
    pub payload: Payload,
    pub patches: Vec<Value>,
    pub allocations: Vec<Row>,
}

impl Writer {
    pub fn new(
        template: &Payload,
        classes: (u32, u32),
        instance_size: usize,
        definition_size: usize,
    ) -> Result<Self> {
        let definition = INSTANCE + instance_size + 8;
        let ti = template.pointer(16)?;
        let td = template.pointer(24)?;
        let mut writer = Self {
            payload: Payload(vec![0; definition + definition_size]),
            patches: Vec::new(),
            allocations: Vec::new(),
        };
        writer.payload.0[..INSTANCE].copy_from_slice(&template.0[..INSTANCE]);
        for (at, target) in [(16, INSTANCE), (24, definition)] {
            put(
                &mut writer.payload.0,
                at,
                &(target as i64 - at as i64).to_le_bytes(),
            )?;
        }
        put(&mut writer.payload.0, 32, &[0; 32])?;
        put(&mut writer.payload.0, 0x44, &OWNER.to_le_bytes())?;
        put(
            &mut writer.payload.0,
            0x48,
            &((definition - INSTANCE) as u64).to_le_bytes(),
        )?;
        writer
            .patches
            .push(json!({"offset":0x44,"symbol":"vehicle-motion-allocation"}));
        put(
            &mut writer.payload.0,
            INSTANCE - 4,
            &classes.0.to_le_bytes(),
        )?;
        put(
            &mut writer.payload.0,
            definition - 4,
            &classes.1.to_le_bytes(),
        )?;
        writer.payload.0[INSTANCE..INSTANCE + 0x30].copy_from_slice(&template.0[ti..ti + 0x30]);
        writer.payload.0[definition..definition + 0x48].copy_from_slice(&template.0[td..td + 0x48]);
        writer.pair(INSTANCE, definition, classes, None)?;
        for field in [0x30, 0x34, 0x48, 0x60, 0x64, 0x70, 0x74] {
            put(
                &mut writer.payload.0,
                INSTANCE + field,
                &u32::MAX.to_le_bytes(),
            )?;
        }
        for field in [0x38, 0x50] {
            put(
                &mut writer.payload.0,
                INSTANCE + field,
                &u64::MAX.to_le_bytes(),
            )?;
        }
        put(
            &mut writer.payload.0,
            INSTANCE + 0x6c,
            &0x811c9dc5u32.to_le_bytes(),
        )?;
        Ok(writer)
    }

    pub fn finish(
        mut self,
        native: &mut Reader,
        graph: &mut Graph,
        initializers: &[(usize, u32, u16)],
        callbacks: Vec<Callback>,
        report: Value,
    ) -> Result<Motion> {
        append(
            &mut self.payload.0,
            48,
            0x808091a4,
            &vec![0; initializers.len() * 16],
            16,
        )?;
        for (&at, &(instance, class, method)) in self
            .payload
            .array(48, 16, Some(0x808091a4))?
            .iter()
            .zip(initializers)
        {
            put(
                &mut self.payload.0,
                at,
                &(instance as i64 - at as i64).to_le_bytes(),
            )?;
            put(&mut self.payload.0, at + 8, &class.to_le_bytes())?;
            put(&mut self.payload.0, at + 12, &method.to_le_bytes())?;
            put(&mut self.payload.0, at + 14, &u16::MAX.to_le_bytes())?;
        }
        let size = self.payload.0.len() as u64;
        put(&mut self.payload.0, 0, &size.to_le_bytes())?;
        let allocation = native.tag(0x80c70b90, Some(0x80809bbb))?;
        ensure!(
            allocation.0.len() == 48 && allocation.u64(32)? == 0,
            "Native constant motion allocation differs"
        );
        let mut allocation = (*allocation).clone();
        if !self.allocations.is_empty() {
            Row::write(&self.allocations, &mut allocation, 32)?;
        }
        graph.add(
            "vehicle-motion-allocation",
            0x80c70b90,
            &allocation.0,
            None,
            vec![],
        )?;
        let mut bytes = self.payload.0.clone();
        for patch in &self.patches {
            put(
                &mut bytes,
                patch["offset"].as_u64().unwrap() as usize,
                &u32::MAX.to_le_bytes(),
            )?;
        }
        graph.add(SYMBOL, 0x80bfaf05, &bytes, None, self.patches)?;
        Ok(Motion {
            payload: self.payload,
            callbacks,
            inputs: Vec::new(),
            retain_hover_update: false,
            report,
        })
    }

    pub fn pair(
        &mut self,
        instance: usize,
        definition: usize,
        classes: (u32, u32),
        parent: Option<usize>,
    ) -> Result<()> {
        for (at, class, target) in [
            (instance, classes.1, definition),
            (definition, classes.0, instance),
        ] {
            put(&mut self.payload.0, at, &OWNER.to_le_bytes())?;
            put(&mut self.payload.0, at + 4, &class.to_le_bytes())?;
            put(&mut self.payload.0, at + 8, &(target as u64).to_le_bytes())?;
            self.patches.push(json!({"offset":at,"symbol":SYMBOL}));
        }
        if let Some(parent) = parent {
            put(
                &mut self.payload.0,
                instance + 16,
                &(parent as i64 - (instance + 16) as i64).to_le_bytes(),
            )?;
        }
        Ok(())
    }

    pub fn scalar(
        &mut self,
        instance: usize,
        definition: usize,
        parent: usize,
        program: Program,
    ) -> Result<()> {
        self.pair(instance, definition, (0x808089f5, 0x808089f6), Some(parent))?;
        put(
            &mut self.payload.0,
            definition + 0x58,
            &0x811c9dc5u64.to_le_bytes(),
        )?;
        program.write(&mut self.payload, definition + 16)
    }

    pub fn provider(
        &mut self,
        graph: &mut Graph,
        slot: usize,
        definition: usize,
        contract: Contract<'_>,
    ) -> Result<Payload> {
        let Contract {
            interface,
            instance_interface,
            implementation,
            methods,
            symbol,
            priority,
        } = contract;
        let mut metadata = Payload(vec![0; 32]);
        put(&mut metadata.0, 8, &interface.to_le_bytes())?;
        put(&mut metadata.0, 12, &instance_interface.to_le_bytes())?;
        let rows = methods
            .iter()
            .flat_map(|index| {
                [
                    implementation.to_le_bytes().as_slice(),
                    index.to_le_bytes().as_slice(),
                    &[0; 16],
                ]
                .concat()
            })
            .collect::<Vec<_>>();
        append(&mut metadata.0, 16, 0x80809c56, &rows, 24)?;
        graph.add(symbol, 0x80bfabcd, &metadata.0, None, vec![])?;
        put(
            &mut self.payload.0,
            slot,
            &(definition as i64 - slot as i64).to_le_bytes(),
        )?;
        // A temporary identity is used only while compiling the entity. The graph
        // linker replaces it before any package is emitted.
        put(&mut self.payload.0, slot + 8, &OWNER.to_le_bytes())?;
        put(&mut self.payload.0, slot + 16, &priority.to_le_bytes())?;
        self.patches.push(json!({"offset":slot+8,"symbol":symbol}));
        Ok(metadata)
    }
}

pub(super) struct Contract<'a> {
    pub interface: u32,
    pub instance_interface: u32,
    pub implementation: u32,
    pub methods: &'a [u32],
    pub symbol: &'a str,
    pub priority: i16,
}

pub(super) fn number(source: &Value, key: &str) -> Result<f32> {
    let value = source[key]
        .as_f64()
        .with_context(|| format!("Source vehicle lacks {key}"))? as f32;
    ensure!(value.is_finite(), "Source vehicle {key} is not finite");
    Ok(value)
}

pub(super) fn build(
    native: &mut Reader,
    graph: &mut Graph,
    source: &Value,
    activation_priority: i16,
) -> Result<Motion> {
    let template = native.tag(0x80bfaf05, Some(0x80809c36))?;
    let ti = template.pointer(16)?;
    let td = template.pointer(24)?;
    ensure!(
        template.u32(ti - 4)? == 0x80803caa
            && template.u32(td - 4)? == 0x80803cb2
            && template.u32(td + 0x370 + 4)? == 0x80803cf7,
        "Native engine envelope differs"
    );
    let mut w = Writer::new(&template, (0x80803ce6, 0x80803cef), 0x300, 0x570)?;

    let steering = &source["steering"];
    let cusp = number(steering, "overdamp_cusp_radians")?;
    let exponent = number(steering, "overdamp_exponent")?;
    let left = number(steering, "left_limit_radians")?;
    let right = number(steering, "right_limit_radians")?;
    let rate = number(steering, "rate_radians_per_second")?;
    ensure!(
        cusp >= 0. && exponent > 0. && left > 0. && right < 0. && rate > 0.,
        "Source steering controls are outside the supported domain"
    );
    w.pair(
        INSTANCE + 0xa0,
        DEFINITION + 0x290,
        (0x80803d80, 0x80803d81),
        Some(INSTANCE),
    )?;
    // Captured 18293F0 converts only the cusp from degrees to radians. The
    // 1829B20/12982A0 limit and rate consumer takes radians and negates right.
    w.scalar(
        INSTANCE + 0xc0,
        DEFINITION + 0x2a0,
        INSTANCE + 0xa0,
        Program::constant(cusp.to_degrees())?,
    )?;
    w.scalar(
        INSTANCE + 0xf0,
        DEFINITION + 0x300,
        INSTANCE + 0xa0,
        Program::constant(exponent)?,
    )?;
    for (index, value) in [left, -right, rate].into_iter().enumerate() {
        put(
            &mut w.payload.0,
            DEFINITION + 0x360 + index * 4,
            &value.to_le_bytes(),
        )?;
    }
    // The alternate control remappers are valid identity functions. Mode zero
    // uses the native direct input path, so these do not invent source shaping.
    for (i, d) in [
        (INSTANCE + 0x120, DEFINITION + 0xf0),
        (INSTANCE + 0x1a0, DEFINITION + 0x1c0),
    ] {
        w.pair(i, d, (0x80803ceb, 0x80803cec), Some(INSTANCE))?;
        w.scalar(i + 0x20, d + 0x10, i, Program::identity())?;
        w.scalar(i + 0x50, d + 0x70, i, Program::identity())?;
    }
    w.payload.0[ENGINE..ENGINE + 0x80].copy_from_slice(&template.0[td + 0x370..td + 0x3f0]);
    w.pair(
        INSTANCE + 0x220,
        ENGINE,
        (0x80803cf7, 0x80803cf8),
        Some(INSTANCE),
    )?;
    let moment = number(source, "engine_moment_source")?;
    let angular_velocity = number(source, "engine_maximum_angular_velocity_source")?;
    let circumference = number(source, "wheel_circumference_source")?;
    ensure!(
        moment > 0. && angular_velocity > 0. && circumference > 0.,
        "Source engine controls must be positive"
    );
    for (field, value) in [
        (0x30, moment),
        (0x34, angular_velocity * std::f32::consts::TAU),
        (0x44, circumference * super::METRES),
    ] {
        put(&mut w.payload.0, ENGINE + field, &value.to_le_bytes())?;
    }
    // EB7400 wraps front and rear travelled distance by this period. The
    // phase getters return zero when it is absent, even while the car moves.
    put(
        &mut w.payload.0,
        DEFINITION + 0x428,
        &(circumference * super::METRES).to_le_bytes(),
    )?;
    w.provider(
        graph,
        ENGINE + 0x10,
        ENGINE,
        Contract {
            interface: 0x80809ae1,
            instance_interface: 0x80809ae2,
            implementation: 0x80803cf7,
            methods: &[1],
            symbol: "vehicle-engine-output",
            priority: 0,
        },
    )?;
    let gears = source["gears"].as_array().context("Source vehicle gears")?;
    ensure!(
        (2..=16).contains(&gears.len()),
        "Source vehicle requires reverse and forward gears"
    );
    append(
        &mut w.payload.0,
        ENGINE + 0x48,
        0x80803cfa,
        &vec![0; gears.len() * 0x88],
        0x88,
    )?;
    for (index, (&at, gear)) in w
        .payload
        .array(ENGINE + 0x48, 0x88, Some(0x80803cfa))?
        .iter()
        .zip(gears)
        .enumerate()
    {
        let ratio = number(gear, "ratio")?;
        ensure!(
            if index == 0 { ratio < 0. } else { ratio > 0. },
            "Source engine gear order is unsupported"
        );
        Program::source(&gear["loaded_torque"])?.write(&mut w.payload, at + 8)?;
        Program::source(&gear["cruising_torque"])?.write(&mut w.payload, at + 0x40)?;
        for (offset, key) in [
            (0x70, "upshift_seconds"),
            (0x74, "upshift_scale"),
            (0x78, "downshift_seconds"),
            (0x7c, "downshift_scale"),
            (0x80, "ratio"),
        ] {
            put(
                &mut w.payload.0,
                at + offset,
                &number(gear, key)?.to_le_bytes(),
            )?;
        }
    }
    w.payload.0[DEFINITION + 0x370..DEFINITION + 0x390]
        .copy_from_slice(&template.0[td + 0x3f8..td + 0x418]);
    w.pair(
        INSTANCE + 0x290,
        DEFINITION + 0x370,
        (0x80806b18, 0x80806b19),
        Some(INSTANCE),
    )?;
    w.pair(
        INSTANCE + 0x2c0,
        DEFINITION + 0x390,
        (0x80806b14, 0x80806b15),
        Some(INSTANCE),
    )?;
    let mut callbacks = Vec::new();
    for (offset, interface, instance_interface, methods, key, priority, condition, symbol) in [
        (
            0x48,
            0x80803ce8,
            0x80803ce7,
            vec![0, 1],
            0xd5bacd4a,
            0,
            0,
            "vehicle-car-interface",
        ),
        (
            0x60,
            0x80809ade,
            0x80809add,
            vec![3],
            0x95a60f29,
            activation_priority,
            0,
            "vehicle-car-activate",
        ),
        (
            0x78,
            0x808097bb,
            0x808097ba,
            vec![4],
            0xab076dc7,
            -7,
            0,
            "vehicle-car-update",
        ),
        (
            0x90,
            0x80809ada,
            0x80809ad9,
            vec![5],
            0x68f68780,
            0,
            0,
            "vehicle-car-release",
        ),
        (
            0xa8,
            0x80803d4b,
            0x80803d4a,
            vec![16, 17],
            0x18dba5e9,
            0,
            0,
            "vehicle-car-motion",
        ),
        (
            0xc0,
            0x80809494,
            0x80809493,
            vec![19],
            0xecc9dfcb,
            0,
            0,
            "vehicle-car-input",
        ),
        (
            0x530,
            0x8080974b,
            0x8080974a,
            vec![14],
            0x394af95a,
            0,
            0,
            "vehicle-car-query",
        ),
        (
            0x550,
            0x8080974b,
            0x8080974a,
            vec![15],
            0x43a02aa1,
            0,
            0,
            "vehicle-car-seat-query",
        ),
    ] {
        let at = DEFINITION + offset;
        let metadata = w.provider(
            graph,
            at,
            DEFINITION,
            Contract {
                interface,
                instance_interface,
                implementation: 0x80803ce6,
                methods: &methods,
                symbol,
                priority,
            },
        )?;
        callbacks.push(Callback {
            key,
            offset: at,
            condition,
            metadata,
        });
    }
    for (index, class) in [(0x530, 0x808092fcu32), (0x550, 0x80803d7b)] {
        put(
            &mut w.payload.0,
            DEFINITION + index + 24,
            &class.to_le_bytes(),
        )?;
    }
    for index in 0..8 {
        let symbol = format!("vehicle-car-output-{index}");
        let slot = DEFINITION + 0x430 + index * 0x20;
        w.provider(
            graph,
            slot,
            DEFINITION,
            Contract {
                interface: 0x80809ae1,
                instance_interface: 0x80809ae2,
                implementation: 0x80803ce6,
                methods: &[6 + index as u32],
                symbol: &symbol,
                priority: 0,
            },
        )?;
        put(&mut w.payload.0, slot + 24, &0x811c9dc5u64.to_le_bytes())?;
    }
    w.finish(
        native, graph,
        &[(INSTANCE + 0x220, 0x80803cf7, 0), (INSTANCE, 0x80803ce6, 2)],
        callbacks,
        json!({
            "owner":SYMBOL,"instance_class":0x80803ce6u32,"definition_class":0x80803cefu32,
            "gears":gears.len(),"source":source,"native_cusp_degrees":cusp.to_degrees(),
            "native_engine_maximum_angular_velocity":angular_velocity * std::f32::consts::TAU,
            "gameplay_verified":false,
            "limits":["Engine angular-unit scaling and bounded source torque interpretation require a source-runtime and gameplay comparison. The native engine response envelope is retained."]
        })
    )
}
