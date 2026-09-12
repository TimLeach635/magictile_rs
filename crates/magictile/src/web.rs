//! Running the tiling viewer in a browser.

use wasm_bindgen::prelude::*;

/// Starts the viewer on a canvas, showing t{p,q} (4 and 5 give the truncated order-5 square
/// tiling). Returns once the app is running; errors if the tiling is not hyperbolic.
#[wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement, p: i32, q: i32) -> Result<(), JsValue> {
    console_error_panic_hook::set_once();
    let tiling = crate::tiling::TruncatedTiling::new(p, q).map_err(|e| JsValue::from_str(&e))?;
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|cc| Ok(Box::new(crate::tiling::TilingApp::new(cc, tiling)))),
        )
        .await
}
