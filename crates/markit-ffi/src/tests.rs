use super::*;

#[test]
fn binary_exports_preserve_plan_and_validate_borrowed_requests() {
    let source = "const 🦀 = 1;";
    let language = "ts";
    let request = MarkitCodeRequest {
        source: source.as_ptr(),
        source_len: source.len(),
        language: language.as_ptr(),
        language_len: language.len(),
        mode: 0,
    };
    let code = unsafe { markit_code_binary(&request) };
    assert_eq!(code.status, 0);
    assert_eq!(
        unsafe { slice::from_raw_parts(code.data, code.len) },
        markit::encode_document_binary(&markit::code_document(source, language)).unwrap()
    );
    unsafe { markit_buffer_free(code) };
    let doc = unsafe { markit_render_binary(source.as_ptr(), source.len()) };
    assert_eq!(doc.status, 0);
    assert_eq!(
        unsafe { slice::from_raw_parts(doc.data, doc.len) },
        markit::render_document_binary(source).unwrap()
    );
    unsafe { markit_buffer_free(doc) };
    for result in [
        unsafe { markit_code_binary(ptr::null()) },
        unsafe { markit_render_binary(ptr::null(), 1) },
        unsafe { markit_render_binary(b"\xff".as_ptr(), 1) },
        unsafe {
            markit_code_binary(&MarkitCodeRequest {
                mode: 99,
                ..request
            })
        },
    ] {
        assert_ne!(result.status, 0);
        unsafe { markit_buffer_free(result) };
    }
    let empty = unsafe { markit_render_binary(ptr::null(), 0) };
    assert_eq!(empty.status, 0);
    unsafe { markit_buffer_free(empty) };
}

#[test]
fn roundtrip_and_free_with_embedded_nul() {
    let source = "# Hello 🦀\0";
    for format in [0, 1] {
        let result = unsafe { markit_render(source.as_ptr(), source.len(), format) };
        assert_eq!(result.status, 0);
        let text = unsafe { slice::from_raw_parts(result.data, result.len) };
        assert!(str::from_utf8(text).unwrap().contains("Hello"));
        unsafe { markit_buffer_free(result) };
    }
}

#[test]
fn rejects_bad_inputs_and_accepts_empty() {
    for (data, len, format, expected) in [
        (ptr::null(), 1, 0, 2),
        (ptr::null(), 0, 99, 2),
        (b"\xff".as_ptr(), 1, 0, 1),
        (ptr::null(), 0, 0, 0),
    ] {
        let buffer = unsafe { markit_render(data, len, format) };
        assert_eq!(buffer.status, expected);
        unsafe { markit_buffer_free(buffer) };
    }
}

#[test]
fn code_requests_and_stream_handles_roundtrip_real_json() {
    let request = br#"{"source":"let x = 1;","language":"rust"}"#;
    let buffer = unsafe { markit_render(request.as_ptr(), request.len(), 2) };
    assert_eq!(buffer.status, 0);
    let document: serde_json::Value =
        serde_json::from_slice(unsafe { slice::from_raw_parts(buffer.data, buffer.len) }).unwrap();
    assert_eq!(document["text"], "let x = 1;");
    assert!(document["spans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|span| span["kind"] == "syntax"));
    unsafe {
        markit_buffer_free(buffer);
    }
    let handle = markit_stream_new();
    for delta in ["First\n\nOpen", " tail\n"] {
        let patch = unsafe { markit_stream_append(handle, delta.as_ptr(), delta.len(), 0) };
        assert_eq!(patch.status, 0);
        let value: serde_json::Value =
            serde_json::from_slice(unsafe { slice::from_raw_parts(patch.data, patch.len) })
                .unwrap();
        assert!(value["blocks"].is_array());
        unsafe {
            markit_buffer_free(patch);
        }
    }
    let binary = unsafe { markit_stream_append_binary(handle, b"\n".as_ptr(), 1, 1) };
    assert_eq!(binary.status, 0);
    let bytes = unsafe { slice::from_raw_parts(binary.data, binary.len) };
    assert_eq!(&bytes[..4], b"MKNP");
    unsafe {
        markit_buffer_free(binary);
        markit_stream_free(handle);
    }
    let error = unsafe { markit_stream_append(ptr::null_mut(), ptr::null(), 0, 1) };
    assert_eq!(error.status, 2);
}

#[test]
fn github_flavour_reaches_renders_binary_plans_and_streams() {
    let source = "<kbd>K</kbd> @octocat\n";
    let read = |buffer: MarkitBuffer| {
        assert_eq!(buffer.status, 0);
        let bytes = unsafe { slice::from_raw_parts(buffer.data, buffer.len) }.to_vec();
        unsafe { markit_buffer_free(buffer) };
        bytes
    };
    let html = read(unsafe { markit_render_flavor(source.as_ptr(), source.len(), 1, 1) });
    assert!(str::from_utf8(&html).unwrap().contains("<kbd>K</kbd>"));
    let plain = read(unsafe { markit_render(source.as_ptr(), source.len(), 1) });
    assert!(str::from_utf8(&plain).unwrap().contains("&lt;kbd&gt;"));
    let json = read(unsafe { markit_render_flavor(source.as_ptr(), source.len(), 0, 1) });
    assert!(str::from_utf8(&json).unwrap().contains("\"mention\""));
    let binary = read(unsafe { markit_render_binary_flavor(source.as_ptr(), source.len(), 1) });
    assert_eq!(
        binary,
        markit::render_document_binary_with(source, markit::Flavor::Github).unwrap()
    );
    for bad in [
        unsafe { markit_render_flavor(source.as_ptr(), source.len(), 0, 9) },
        unsafe { markit_render_binary_flavor(source.as_ptr(), source.len(), 9) },
    ] {
        assert_eq!(bad.status, 2);
        unsafe { markit_buffer_free(bad) };
    }
    assert!(markit_stream_new_flavor(9).is_null());
    let handle = markit_stream_new_flavor(1);
    let patch = read(unsafe { markit_stream_append(handle, source.as_ptr(), source.len(), 1) });
    assert!(str::from_utf8(&patch).unwrap().contains("\"html\""));
    unsafe { markit_stream_free(handle) };
}

#[test]
fn agent_flavour_code_draws_footnotes_but_leaves_mentions_as_text() {
    let source = "Note[^1] for @octocat #12\n\n[^1]: Yes.\n";
    let read = |buffer: MarkitBuffer| {
        assert_eq!(buffer.status, 0);
        let bytes = unsafe { slice::from_raw_parts(buffer.data, buffer.len) }.to_vec();
        unsafe { markit_buffer_free(buffer) };
        String::from_utf8(bytes).unwrap()
    };
    let html = read(unsafe { markit_render_flavor(source.as_ptr(), source.len(), 1, 2) });
    assert!(html.contains("mk-footnotes"), "{html}");
    assert!(html.contains("@octocat #12"), "{html}");
    assert!(!html.contains("mk-mention"), "{html}");
    let json = read(unsafe { markit_render_flavor(source.as_ptr(), source.len(), 0, 2) });
    assert!(json.contains("\"footnote_reference\"") && !json.contains("\"mention\""));
    let handle = markit_stream_new_flavor(2);
    assert!(!handle.is_null());
    let patch = read(unsafe { markit_stream_append(handle, source.as_ptr(), source.len(), 1) });
    assert!(patch.contains("footnote_reference") && !patch.contains("\"mention\""));
    unsafe { markit_stream_free(handle) };
}

#[test]
fn patch_stream_sends_fence_extensions() {
    let handle = markit_stream_new();
    let mut kinds = Vec::new();
    for delta in ["```rust\n", "let a = 1;\n", "let b = 2;\n", "let c = 3;\n"] {
        let patch = unsafe { markit_stream_append_patch(handle, delta.as_ptr(), delta.len(), 0) };
        assert_eq!(patch.status, 0);
        let bytes = unsafe { slice::from_raw_parts(patch.data, patch.len) };
        assert_eq!(&bytes[..8], b"MKNP\x02\0\0\0");
        let count = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        if count > 0 {
            kinds.push(u32::from_le_bytes(bytes[28..32].try_into().unwrap()));
        }
        unsafe { markit_buffer_free(patch) };
    }
    assert_eq!(kinds.last(), Some(&1));
    let bad = [0xffu8];
    let invalid = unsafe { markit_stream_append_patch(handle, bad.as_ptr(), 1, 0) };
    assert_eq!(invalid.status, 1);
    unsafe { markit_stream_free(handle) };
    let error = unsafe { markit_stream_append_patch(ptr::null_mut(), ptr::null(), 0, 1) };
    assert_eq!(error.status, 2);
}
