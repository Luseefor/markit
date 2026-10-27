use wasm_bindgen::prelude::*;

#[wasm_bindgen(js_name = renderHtml)]
pub fn render_html(source: &str) -> String {
    markit::render_html(source)
}

#[wasm_bindgen(js_name = renderDocument)]
pub fn render_document(source: &str) -> String {
    markit::render_document_json(source)
}
