//! The observable contract is which scenario identities may be removed from permanent geometry.
//! These small source programs exercise call-frame combinations absent from a single map witness.
use super::super::{
    Node, Parameter, Program, Script, Tag, analysis, binding, validate, vocabulary,
};
use anyhow::Result;
use std::collections::BTreeSet;

#[derive(Default)]
struct Source {
    nodes: Vec<Option<Node>>,
    scripts: Vec<Script>,
    named: Vec<binding::Named>,
}

impl Source {
    fn node(&mut self, opcode: u16, ty: u16, kind: u16, value: u32) -> u32 {
        let datum = 0x1234_0000 | self.nodes.len() as u32;
        self.nodes.push(Some(Node {
            datum,
            opcode,
            value_type: ty,
            flags: kind,
            next: None,
            source_offset: 0,
            value,
            line: 1,
            padding: 0,
            text_bytes: None,
            text: None,
        }));
        datum
    }

    fn object(&mut self, index: u16) -> u32 {
        let datum = self.node(78, 78, 9, u32::from(index));
        self.named.push(binding::Named {
            expression: datum,
            value_type: 78,
            value: u32::from(index),
            domain: "object_name",
            index: u32::from(index),
            name: Some(format!("object_{index}")),
        });
        datum
    }

    fn parameter(&mut self, index: u32) -> u32 {
        self.node(78, 78, 29, index)
    }

    fn call(&mut self, opcode: u16, kind: u16, args: &[u32]) -> u32 {
        let head = self.node(opcode, 2, 9, 0);
        let chain = std::iter::once(head)
            .chain(args.iter().copied())
            .collect::<Vec<_>>();
        for pair in chain.windows(2) {
            self.nodes[(pair[0] & 0xffff) as usize]
                .as_mut()
                .unwrap()
                .next = Some(pair[1]);
        }
        self.node(opcode, 4, kind, head)
    }

    fn operation(&mut self, name: &str, args: &[u32]) -> u32 {
        let opcode = *vocabulary::get()
            .functions
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .unwrap()
            .0;
        self.call(opcode, 8, args)
    }

    fn script(&mut self, parameters: usize, body: &[u32]) {
        let root = self.operation("begin", body);
        self.scripts.push(Script {
            name_id: 0,
            name: format!("script_{}", self.scripts.len()),
            kind: 3,
            return_type: 4,
            root: Some(root),
            parameters: (0..parameters)
                .map(|i| Parameter {
                    name: format!("arg_{i}"),
                    value_type: 78,
                })
                .collect(),
        });
    }

    fn finish(self) -> Result<Program> {
        let mut program = Program {
            schema: 1,
            source_build: "Jun 21 2023 15:35:31".into(),
            source_metadata_sha256: "0".repeat(64),
            scenario: Tag {
                datum: 0x1234_ffff,
                group: "scnr".into(),
                path: "fixture".into(),
                offset: None,
            },
            execution: "unsupported",
            scripts: self.scripts,
            globals: Vec::new(),
            nodes: self.nodes,
            tag_references: Vec::new(),
            bindings: binding::Bindings {
                named: self.named,
                tags: Vec::new(),
            },
            analysis: Default::default(),
            audio: serde_json::Value::Null,
        };
        validate(&program)?;
        program.analysis = analysis::read(&program)?;
        Ok(program)
    }
}

#[test]
fn forwarded_lifetimes_keep_call_frames_and_ignore_unknown_values() -> Result<()> {
    let mut source = Source::default();
    // create(first, second) creates first, then calls destroy(second, first).
    let first = source.parameter(0);
    let create = source.operation("object_create", &[first]);
    let second = source.parameter(1);
    let first = source.parameter(0);
    let forward = source.call(1, 10, &[second, first]);
    source.script(2, &[create, forward]);
    // destroy(first, second) destroys second, then recurses with the arguments swapped.
    let second = source.parameter(1);
    let destroy = source.operation("object_destroy", &[second]);
    let second = source.parameter(1);
    let first = source.parameter(0);
    let recurse = source.call(0, 10, &[second, first]);
    source.script(2, &[destroy, recurse]);

    let mut body = Vec::new();
    for index in [40, 41] {
        let target = source.object(index);
        let untouched = source.object(42);
        body.push(source.call(0, 10, &[target, untouched]));
    }
    // The numeric value happens to equal a named object, but it has no object-name binding.
    let unknown = source.node(8, 8, 9, 43);
    let untouched = source.object(42);
    body.push(source.call(0, 10, &[unknown, untouched]));
    let object = source.object(43);
    body.push(source.operation("object_create", &[object]));
    // Direct lifetimes still work, while single-sided lifetimes stay in the map.
    for operation in ["object_create_if_necessary", "object_destroy"] {
        let object = source.object(44);
        body.push(source.operation(operation, &[object]));
    }
    let object = source.object(45);
    body.push(source.operation("object_destroy", &[object]));
    source.script(0, &body);

    let result = super::read(&source.finish()?)?;
    assert_eq!(
        result.transient.keys().copied().collect::<BTreeSet<_>>(),
        BTreeSet::from([40, 41, 44])
    );
    assert!(
        !result.unresolved.is_empty(),
        "Unknown object arguments must remain inspectable"
    );
    for lifetime in result.transient.values() {
        assert!(!lifetime.create.is_empty() && !lifetime.destroy.is_empty());
        assert_eq!(
            lifetime.create.len(),
            lifetime.create.iter().collect::<BTreeSet<_>>().len()
        );
        assert_eq!(
            lifetime.destroy.len(),
            lifetime.destroy.iter().collect::<BTreeSet<_>>().len()
        );
    }
    Ok(())
}

#[test]
fn different_parameter_slots_do_not_form_a_false_lifetime() -> Result<()> {
    let mut source = Source::default();
    let first = source.parameter(0);
    let create = source.operation("object_create_anew", &[first]);
    let second = source.parameter(1);
    let destroy = source.operation("object_destroy", &[second]);
    source.script(2, &[create, destroy]);
    let first = source.object(10);
    let second = source.object(11);
    let call = source.call(0, 10, &[first, second]);
    source.script(0, &[call]);
    assert!(super::read(&source.finish()?)?.transient.is_empty());
    Ok(())
}

#[test]
fn malformed_helper_calls_fail_instead_of_guessing_a_binding() -> Result<()> {
    let mut source = Source::default();
    let parameter = source.parameter(0);
    let create = source.operation("object_create", &[parameter]);
    source.script(1, &[create]);
    let call = source.call(0, 10, &[]);
    source.script(0, &[call]);
    assert!(super::read(&source.finish()?).is_err());
    Ok(())
}

#[test]
fn assigned_parameters_are_not_assumed_to_keep_their_call_argument() -> Result<()> {
    let mut source = Source::default();
    let parameter = source.parameter(0);
    let create = source.operation("object_create", &[parameter]);
    let target = source.parameter(0);
    let replacement = source.object(21);
    let assign = source.operation("set", &[target, replacement]);
    let parameter = source.parameter(0);
    let destroy = source.operation("object_destroy", &[parameter]);
    source.script(1, &[create, assign, destroy]);
    let object = source.object(20);
    let call = source.call(0, 10, &[object]);
    source.script(0, &[call]);
    assert!(super::read(&source.finish()?)?.transient.is_empty());
    Ok(())
}
