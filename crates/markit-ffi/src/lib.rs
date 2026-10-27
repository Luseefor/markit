use std::{panic::catch_unwind, ptr, slice, str};

#[cfg(target_os = "android")]
mod android;

/// Owned UTF-8 bytes. Release exactly once with markit_buffer_free.
/// status: 0 success, 1 invalid UTF-8, 2 invalid argument, 3 internal panic.
#[repr(C)]
pub struct MarkitBuffer {
    pub data: *mut u8,
    pub len: usize,
    pub status: u32,
}

impl MarkitBuffer {
    fn error(status: u32) -> Self {
        Self {
            data: ptr::null_mut(),
            len: 0,
            status,
        }
    }
    fn owned(text: String) -> Self {
        let bytes = text.into_bytes().into_boxed_slice();
        let len = bytes.len();
        Self {
            data: Box::into_raw(bytes) as *mut u8,
            len,
            status: 0,
        }
    }
}

/// format 0 = native JSON, format 1 = safe HTML. Embedded NUL is supported.
///
/// # Safety
/// For nonzero len, source must point to len readable bytes for the duration of
/// this call. len must not exceed isize::MAX. No pointer is retained.
#[no_mangle]
pub unsafe extern "C" fn markit_render(source: *const u8, len: usize, format: u32) -> MarkitBuffer {
    if (source.is_null() && len != 0) || len > isize::MAX as usize || format > 1 {
        return MarkitBuffer::error(2);
    }
    catch_unwind(|| {
        let bytes = if len == 0 {
            &[][..]
        } else {
            // SAFETY: the caller guarantees source is readable for len bytes.
            unsafe { slice::from_raw_parts(source, len) }
        };
        let Ok(text) = str::from_utf8(bytes) else {
            return MarkitBuffer::error(1);
        };
        MarkitBuffer::owned(if format == 0 {
            markit::render_document_json(text)
        } else {
            markit::render_html(text)
        })
    })
    .unwrap_or_else(|_| MarkitBuffer::error(3))
}

/// # Safety
/// Pass an unmodified buffer returned by markit_render, exactly once. Null error
/// buffers are accepted. Copies of a buffer do not transfer additional ownership.
#[no_mangle]
pub unsafe extern "C" fn markit_buffer_free(buffer: MarkitBuffer) {
    if !buffer.data.is_null() {
        // SAFETY: same pointer and length returned from Box::into_raw in owned().
        drop(unsafe { Box::from_raw(ptr::slice_from_raw_parts_mut(buffer.data, buffer.len)) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
