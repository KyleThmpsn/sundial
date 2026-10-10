//! Saved files, resolved texture references and cancelled requests through the UI queue.
use super::*;
use sha2::{Digest, Sha256};
use std::{io::Read, sync::mpsc, time::Instant};

fn model(charts: bool) -> Model {
    let mut model = Model {
        textures: vec![
            texture(1, &[[80, 140, 210, 255], [190, 40, 85, 255]]),
            texture(2, &[[180, 90, 0, 30], [240, 180, 190, 210]]),
            texture(3, &[[170, 100, 230, 255], [100, 160, 190, 255]]),
            texture(4, &[[110, 90, 180, 230], [210, 190, 80, 90]]),
        ],
        ..Model::default()
    };
    model.textures[0].size = [2, 2];
    model.textures[0]
        .rgba
        .extend_from_slice(&[40, 190, 75, 255, 160, 70, 210, 255]);
    quad(&mut model, 0.0, Some(0), None);
    model.triangle_gearstacks = vec![Some(1); 2];
    model.triangle_normals = vec![Some(2); 2];
    model.triangle_clip = vec![true; 2];
    model.dyes[0] = Some(shader::Dye {
        detail: Some(3),
        normal: Some(2),
        ..dye()
    });
    model.dyes[0].as_mut().unwrap().surface.emissive = [1.5, 0.5, 0.2];
    if charts {
        model.uvs.iter_mut().for_each(|uv| uv[0] *= 2.0);
        model.detail_uvs = vec![[0.1, 0.0], [0.8, 0.0], [0.8, 0.9], [0.1, 0.9]];
        model.triangle_detail_uv = vec![true; 2];
    }
    model
}

fn png(bytes: &[u8]) -> ([usize; 2], Vec<u8>) {
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let read = |at| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let size = [read(16), read(20)];
    let channels = match bytes[25] {
        2 => 3,
        6 => 4,
        _ => panic!("unexpected PNG format"),
    };
    let mut compressed = Vec::new();
    let mut at = 8;
    while at + 12 <= bytes.len() {
        let length = read(at);
        if &bytes[at + 4..at + 8] == b"IDAT" {
            compressed.extend_from_slice(&bytes[at + 8..at + 8 + length]);
        }
        at += length + 12;
    }
    let mut raw = Vec::new();
    flate2::read::ZlibDecoder::new(compressed.as_slice())
        .read_to_end(&mut raw)
        .unwrap();
    let stride = size[0] * channels;
    assert_eq!(raw.len(), (stride + 1) * size[1]);
    let pixels = raw
        .chunks_exact(stride + 1)
        .flat_map(|row| {
            assert_eq!(row[0], 0, "Sundial PNG files retain unfiltered rows");
            row[1..].iter().copied()
        })
        .collect();
    (size, pixels)
}

fn glb(bytes: &[u8]) -> (serde_json::Value, &[u8]) {
    assert_eq!(&bytes[..4], b"glTF");
    let read = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    assert_eq!(read(8), bytes.len());
    assert_eq!(&bytes[16..20], b"JSON");
    let end = 20 + read(12);
    assert_eq!(&bytes[end + 4..end + 8], b"BIN\0");
    (
        serde_json::from_slice(&bytes[20..end]).unwrap(),
        &bytes[end + 8..end + 8 + read(end)],
    )
}

fn readback(cpu: &[u8], gpu: &[u8]) -> serde_json::Value {
    let (a, ab) = glb(cpu);
    let (b, bb) = glb(gpu);
    assert_eq!(a["meshes"], b["meshes"]);
    assert_eq!(a["accessors"], b["accessors"]);
    let slice = |doc: &serde_json::Value, index: usize| {
        let view = &doc["bufferViews"][index];
        let first = view["byteOffset"].as_u64().unwrap_or(0) as usize;
        first..first + view["byteLength"].as_u64().unwrap() as usize
    };
    // Follow the actual accessors, rather than assuming a position in the BIN chunk.
    for accessor in a["accessors"].as_array().unwrap() {
        let index = accessor["bufferView"].as_u64().unwrap() as usize;
        assert_eq!(&ab[slice(&a, index)], &bb[slice(&b, index)]);
    }
    let mut results = Vec::new();
    assert_eq!(
        a["materials"].as_array().unwrap().len(),
        b["materials"].as_array().unwrap().len()
    );
    for (material_index, material) in a["materials"].as_array().unwrap().iter().enumerate() {
        assert_eq!(
            material["alphaMode"],
            b["materials"][material_index]["alphaMode"]
        );
        assert_eq!(
            material["doubleSided"],
            b["materials"][material_index]["doubleSided"]
        );
        for path in [
            "/pbrMetallicRoughness/baseColorTexture/index",
            "/pbrMetallicRoughness/metallicRoughnessTexture/index",
            "/normalTexture/index",
            "/emissiveTexture/index",
        ] {
            let texture = material.pointer(path).and_then(serde_json::Value::as_u64);
            let other = b["materials"][material_index]
                .pointer(path)
                .and_then(serde_json::Value::as_u64);
            assert_eq!(texture.is_some(), other.is_some());
            let Some(texture) = texture else {
                continue;
            };
            let image = |doc: &serde_json::Value, data: &[u8], texture: u64| {
                let source = doc["textures"][texture as usize]["source"]
                    .as_u64()
                    .unwrap() as usize;
                let view = doc["images"][source]["bufferView"].as_u64().unwrap() as usize;
                png(&data[slice(doc, view)])
            };
            let (asize, apixels) = image(&a, ab, texture);
            let (bsize, bpixels) = image(&b, bb, other.unwrap());
            assert_eq!(asize, bsize);
            assert_eq!(apixels.len(), bpixels.len());
            let error = apixels
                .iter()
                .zip(&bpixels)
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(error <= 3, "{path} differs by {error}");
            results.push(json!({"role":path,"size":asize,"max_channel_difference":error}));
        }
    }
    assert!(!results.is_empty());
    json!({"geometry_equal":true,"images":results})
}

#[test]
#[ignore = "Opens a native OpenGL viewport, requires SUNDIAL_PREVIEW_VERIFY_OUTPUT"]
fn saved_exports_use_the_gpu_and_cancel_on_close() {
    let output = PathBuf::from(std::env::var_os("SUNDIAL_PREVIEW_VERIFY_OUTPUT").expect("output"))
        .join("exports");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("Use fresh verification output");
    let (sender, receiver) = mpsc::channel();
    eframe::run_native(
        "Preview Export Verification",
        eframe::NativeOptions {
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([260.0, 240.0])
                .with_active(false),
            event_loop_builder: Some(Box::new(crate::test_support::native_event_loop)),
            ..Default::default()
        },
        Box::new(move |creation| {
            assert!(creation.gl.is_some());
            let shared = Shared::default();
            let client = shared.baker(creation.egui_ctx.clone(), Arc::new(model(false)));
            shared.cancel_exports();
            let worker_shared = shared.clone();
            let context = creation.egui_ctx.clone();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut cancelled = client;
                    assert!(export::glb_using(&model(false), 0.0, Some(&mut cancelled)).is_err());
                    let mut receipts = Vec::new();
                    for charts in [false, true] {
                        let model = Arc::new(model(charts));
                        let mut baker = worker_shared.baker(context.clone(), model.clone());
                        let cpu = export::glb(&model, 0.0).unwrap();
                        let gpu = export::glb_using(&model, 0.0, Some(&mut baker)).unwrap();
                        fs::write(output.join(format!("charts-{charts}-cpu.glb")), &cpu).unwrap();
                        fs::write(output.join(format!("charts-{charts}-gpu.glb")), &gpu).unwrap();
                        receipts.push(readback(&cpu, &gpu));
                        let frame = Frame {
                            model: model.clone(),
                            camera: Camera {
                                yaw: 0.0,
                                pitch: 0.0,
                                zoom: 1.0,
                                pan: [0.0; 2],
                            },
                            scene: Scene::unprocessed(),
                            style: Style::Textured,
                            seconds: 0.0,
                            pose: None,
                            animate: true,
                            dyes: None,
                        };
                        let cpu = render::styled_image(
                            &model,
                            frame.camera,
                            frame.scene,
                            [320, 240],
                            0.0,
                            frame.style,
                        );
                        let [r, g, b] = frame.scene.background;
                        let background = egui::Color32::from_rgb(r, g, b);
                        let cancelled = worker_shared.image(context.clone(), Frame {
                            model: model.clone(), camera: frame.camera, scene: frame.scene,
                            style: frame.style, seconds: frame.seconds, animate: true, dyes: None, pose: None,
                        }, [320, 240]);
                        worker_shared.cancel_exports();
                        assert!(cancelled.wait().is_err(), "Closing must release a queued image receiver");
                        let image = worker_shared
                            .image(context.clone(), frame, [320, 240])
                            .wait()
                            .unwrap();
                        assert_eq!(image.size, [320, 240]);
                        let rgba: Vec<_> = image
                            .pixels
                            .iter()
                            .flat_map(egui::Color32::to_array)
                            .collect();
                        let encoded = export::png(&rgba, 320, 240).unwrap();
                        assert_eq!(png(&encoded).1, rgba);
                        fs::write(output.join(format!("charts-{charts}-gpu.png")), encoded)
                            .unwrap();
                        let mut compared = 0;
                        for y in 3..237 {
                            for x in 3..317 {
                                if !interior(&cpu, x, y, background) || !interior(&image, x, y, background) { continue; }
                                let at = y * 320 + x;
                                for (a, b) in cpu.pixels[at]
                                    .to_array()
                                    .into_iter()
                                    .zip(image.pixels[at].to_array())
                                {
                                    assert!(a.abs_diff(b) <= 3);
                                }
                                compared += 1;
                            }
                        }
                        assert!(compared > 100, "Saved images must contain visible material");
                    }
                    let revision = std::process::Command::new("git").args(["rev-parse", "HEAD"]).output().unwrap();
                    assert!(revision.status.success());
                    let artifacts: Vec<_> = fs::read_dir(&output).unwrap().map(|entry| {
                        let path = entry.unwrap().path();
                        json!({"file":path.file_name().unwrap().to_string_lossy(),"sha256":hex::encode(Sha256::digest(fs::read(&path).unwrap()))})
                    }).collect();
                    fs::write(
                        output.join("readback.json"),
                        serde_json::to_vec_pretty(
                            &json!({"cancelled_client_rejected":true,"queued_image_cancelled":true,"glb":receipts,
                                "revision":String::from_utf8_lossy(&revision.stdout).trim(),
                                "executable_sha256":hex::encode(Sha256::digest(fs::read(std::env::current_exe().unwrap()).unwrap())),
                                "artifacts":artifacts,"inputs":"Generated asymmetric plate, gear, normal and detail textures in exports::model",
                                "repeat_filter":"model_preview::gpu::verification::exports::saved_exports_use_the_gpu_and_cancel_on_close",
                                "limits":"GPU queue and saved files compared with the retained CPU exporter. Color quantization tolerance is three byte values. No native asset, gameplay, or performance claim."}),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                }))
                .is_ok();
                sender.send(result).unwrap();
                context.send_viewport_cmd(egui::ViewportCommand::Close);
            });
            Ok(Box::new(ExportApp {
                shared,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert!(
        receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap(),
        "Inspect the retained export files"
    );
}

struct ExportApp {
    shared: Shared,
    started: Instant,
}
impl eframe::App for ExportApp {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        assert!(
            self.started.elapsed().as_secs() < 120,
            "Export queue stalled"
        );
        self.shared.service(ui);
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(5));
    }
}
