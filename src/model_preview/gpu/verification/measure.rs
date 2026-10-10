//! GPU queries belong only to verification, where readback already requires completion.
use super::super::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(in crate::model_preview::gpu) struct Metrics {
    pub pose_upload: Duration,
    pub ordering: Duration,
    pub draw_calls: usize,
    pub indexed_calls: usize,
    pub sorted: bool,
    pub pose_uploaded: bool,
}

pub(super) unsafe fn draw(
    gl: &glow::Context,
    state: &mut State,
    info: &egui::PaintCallbackInfo,
    frame: &Frame,
) -> Option<Value> {
    // SAFETY: the caller is a paint callback with the current context. This native test never
    // binds a query-result buffer, so the 64-bit query destination is an ordinary live pointer.
    unsafe {
        let timer = gl.create_query().expect("GPU timer query");
        gl.begin_query(glow::TIME_ELAPSED, timer);
        let started = Instant::now();
        let ready = state.draw(gl, info, frame);
        let submitted = started.elapsed();
        gl.end_query(glow::TIME_ELAPSED);
        if !ready {
            gl.delete_query(timer);
            return None;
        }
        let waiting = Instant::now();
        let mut gpu_nanoseconds = 0_u64;
        gl.get_query_parameter_u64_with_offset(
            timer,
            glow::QUERY_RESULT,
            (&mut gpu_nanoseconds as *mut u64) as usize,
        );
        let wait = waiting.elapsed();
        gl.delete_query(timer);
        let metrics = &state.metrics;
        Some(json!({
            "cpu_submission_ms":submitted.as_secs_f64()*1000.0,
            "gpu_elapsed_ms":gpu_nanoseconds as f64/1_000_000.0,
            "query_wait_ms":wait.as_secs_f64()*1000.0,
            "pose_upload_ms":metrics.pose_upload.as_secs_f64()*1000.0,
            "ordering_ms":metrics.ordering.as_secs_f64()*1000.0,
            "draw_calls":metrics.draw_calls,"indexed_calls":metrics.indexed_calls,
            "draw_scope":"Model geometry, excluding output passes and egui",
            "order_rebuilt":metrics.sorted,"pose_uploaded":metrics.pose_uploaded,
            "legacy_order":state.legacy_order,
            "limits":"CPU submission includes driver work. GPU elapsed is the command-stream interval, including possible GPU idle time while the CPU submits. Query wait is separate. These are render measurements, not interactive FPS."
        }))
    }
}
