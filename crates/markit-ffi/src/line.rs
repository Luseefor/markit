use std::{panic::catch_unwind, ptr, slice};

use crate::{input, MarkitBuffer};

/// Borrowed lines for [`markit_line_highlighter_edit`]. Each line is UTF-8
/// without its line ending; `lines` and `lengths` hold `count` entries.
#[repr(C)]
pub struct MarkitLineEdit {
    pub start_line: usize,
    pub removed_lines: usize,
    pub lines: *const *const u8,
    pub lengths: *const usize,
    pub count: usize,
}

/// Opaque incremental highlighter. Free with markit_line_highlighter_free.
pub struct MarkitLineHighlighter {
    #[cfg(feature = "grammars")]
    inner: markit::LineHighlighter,
}

/// Creates an incremental highlighter for a language, or null for invalid
/// UTF-8, a null language, or a build without full grammars. Free exactly
/// once with markit_line_highlighter_free.
///
/// # Safety
/// For nonzero len, language must point to len readable UTF-8 bytes for the
/// duration of this call. No pointer is retained.
#[no_mangle]
pub unsafe extern "C" fn markit_line_highlighter_new(
    language: *const u8,
    language_len: usize,
) -> *mut MarkitLineHighlighter {
    let Ok(language) = (unsafe { input(language, language_len) }) else {
        return ptr::null_mut();
    };
    #[cfg(feature = "grammars")]
    {
        catch_unwind(|| {
            Box::into_raw(Box::new(MarkitLineHighlighter {
                inner: markit::LineHighlighter::new(language),
            }))
        })
        .unwrap_or(ptr::null_mut())
    }
    #[cfg(not(feature = "grammars"))]
    {
        let _ = language;
        ptr::null_mut()
    }
}

/// Replaces the highlighter's whole text.
///
/// # Safety
/// handle must come from markit_line_highlighter_new and be used by one caller
/// at a time. source has the same requirements as markit_render.
#[no_mangle]
pub unsafe extern "C" fn markit_line_highlighter_set_text(
    handle: *mut MarkitLineHighlighter,
    source: *const u8,
    len: usize,
) -> u32 {
    if handle.is_null() {
        return 2;
    }
    let source = match unsafe { input(source, len) } {
        Ok(source) => source,
        Err(status) => return status,
    };
    catch_unwind(|| {
        #[cfg(feature = "grammars")]
        {
            // SAFETY: the caller owns a live handle used by one caller at a time.
            unsafe { &mut (*handle).inner }.set_text(source);
            0
        }
        #[cfg(not(feature = "grammars"))]
        {
            let _ = (source, handle);
            2
        }
    })
    .unwrap_or(3)
}

/// Replaces `removed_lines` lines at `start_line` with the edit's lines.
///
/// # Safety
/// Same handle requirements as markit_line_highlighter_set_text. edit must
/// point to a readable MarkitLineEdit whose arrays stay readable during the
/// call and whose lines are UTF-8.
#[no_mangle]
pub unsafe extern "C" fn markit_line_highlighter_edit(
    handle: *mut MarkitLineHighlighter,
    edit: *const MarkitLineEdit,
) -> u32 {
    if handle.is_null() || edit.is_null() {
        return 2;
    }
    catch_unwind(|| {
        // SAFETY: the caller guarantees a readable struct for this call.
        let edit = unsafe { &*edit };
        if edit.count > 0 && (edit.lines.is_null() || edit.lengths.is_null()) {
            return 2;
        }
        #[cfg(feature = "grammars")]
        {
            let pointers = if edit.count == 0 {
                &[][..]
            } else {
                // SAFETY: the caller guarantees count readable entries.
                unsafe { slice::from_raw_parts(edit.lines, edit.count) }
            };
            let lengths = if edit.count == 0 {
                &[][..]
            } else {
                unsafe { slice::from_raw_parts(edit.lengths, edit.count) }
            };
            let mut lines = Vec::with_capacity(edit.count);
            for (line, len) in pointers.iter().zip(lengths) {
                match unsafe { input(*line, *len) } {
                    Ok(line) => lines.push(line),
                    Err(status) => return status,
                }
            }
            // SAFETY: the caller owns a live handle used by one caller at a time.
            unsafe { &mut (*handle).inner }.edit(edit.start_line, edit.removed_lines, &lines);
            0
        }
        #[cfg(not(feature = "grammars"))]
        {
            let _ = (handle, edit);
            2
        }
    })
    .unwrap_or(3)
}

/// JSON array of token arrays for `start_line..end_line`, one per line. Each
/// token has `text`, `light` and `dark`, and lines carry no ending.
///
/// # Safety
/// Same handle requirements as markit_line_highlighter_set_text. Free the
/// result with markit_buffer_free.
#[no_mangle]
pub unsafe extern "C" fn markit_line_highlighter_tokens(
    handle: *mut MarkitLineHighlighter,
    start_line: usize,
    end_line: usize,
) -> MarkitBuffer {
    if handle.is_null() {
        return MarkitBuffer::error(2);
    }
    catch_unwind(|| {
        #[cfg(feature = "grammars")]
        {
            // SAFETY: the caller owns a live handle used by one caller at a time.
            let tokens = unsafe { &mut (*handle).inner }.tokens(start_line, end_line);
            match serde_json::to_string(&tokens) {
                Ok(json) => MarkitBuffer::owned(json),
                Err(_) => MarkitBuffer::error(2),
            }
        }
        #[cfg(not(feature = "grammars"))]
        {
            let _ = (handle, start_line, end_line);
            MarkitBuffer::error(2)
        }
    })
    .unwrap_or_else(|_| MarkitBuffer::error(3))
}

/// # Safety
/// Free a handle returned by markit_line_highlighter_new exactly once, with no
/// call in flight.
#[no_mangle]
pub unsafe extern "C" fn markit_line_highlighter_free(handle: *mut MarkitLineHighlighter) {
    if !handle.is_null() {
        drop(unsafe { Box::from_raw(handle) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markit_buffer_free;
    use std::slice;

    #[test]
    fn line_highlighter_handles_tokens_and_edits() {
        let language = b"rust";
        let handle = unsafe { markit_line_highlighter_new(language.as_ptr(), language.len()) };
        #[cfg(not(feature = "grammars"))]
        {
            assert!(handle.is_null());
            let tokens = unsafe { markit_line_highlighter_tokens(handle, 0, 1) };
            assert_eq!(tokens.status, 2);
            unsafe { markit_line_highlighter_free(handle) };
        }
        #[cfg(feature = "grammars")]
        {
            assert!(!handle.is_null());
            let read = |buffer: MarkitBuffer| -> serde_json::Value {
                assert_eq!(buffer.status, 0);
                let bytes = unsafe { slice::from_raw_parts(buffer.data, buffer.len) }.to_vec();
                unsafe { markit_buffer_free(buffer) };
                serde_json::from_slice(&bytes).unwrap()
            };
            let source = "fn main() {\n    let value = 1;\n}\n";
            assert_eq!(
                unsafe { markit_line_highlighter_set_text(handle, source.as_ptr(), source.len()) },
                0
            );
            let tokens = read(unsafe { markit_line_highlighter_tokens(handle, 0, 1) });
            let first: String = tokens[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|token| token["text"].as_str().unwrap())
                .collect();
            assert_eq!(first, "fn main() {");
            let second = "    let changed = true;";
            let pointers = [second.as_ptr()];
            let lengths = [second.len()];
            let edit = MarkitLineEdit {
                start_line: 1,
                removed_lines: 1,
                lines: pointers.as_ptr(),
                lengths: lengths.as_ptr(),
                count: 1,
            };
            assert_eq!(unsafe { markit_line_highlighter_edit(handle, &edit) }, 0);
            let tokens = read(unsafe { markit_line_highlighter_tokens(handle, 0, 3) });
            let changed: String = tokens[1]
                .as_array()
                .unwrap()
                .iter()
                .map(|token| token["text"].as_str().unwrap())
                .collect();
            assert_eq!(changed, second);
            assert_eq!(
                unsafe { markit_line_highlighter_edit(handle, ptr::null()) },
                2
            );
            unsafe { markit_line_highlighter_free(handle) };
        }
    }
}
