use jni::{
    objects::{JClass, JString},
    sys::{jboolean, jbyteArray, jint, jlong, jstring},
    JNIEnv,
};
#[cfg(feature = "grammars")]
use std::cell::RefCell;
use std::{
    collections::HashMap,
    ptr,
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

type Streams = HashMap<i64, Arc<Mutex<markit::StreamRenderer>>>;
static STREAMS: OnceLock<Mutex<Streams>> = OnceLock::new();
static NEXT: AtomicI64 = AtomicI64::new(1);
fn streams() -> &'static Mutex<Streams> {
    STREAMS.get_or_init(|| Mutex::new(HashMap::new()))
}
fn invoke(
    env: &mut JNIEnv,
    operation: impl FnOnce(&mut JNIEnv) -> Result<String, String>,
) -> jstring {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(env)));
    let text = match result {
        Ok(Ok(text)) => text,
        other => {
            let error = match other {
                Ok(Err(error)) => error,
                _ => "Markit rendering failed".into(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/IllegalStateException", error);
            }
            return ptr::null_mut();
        }
    };
    env.new_string(text)
        .map(|text| text.into_raw())
        .unwrap_or(ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeRender(
    mut env: JNIEnv,
    _: JClass,
    source: JString,
    format: jint,
) -> jstring {
    render(&mut env, source, format, 0)
}
/// `flavor`: 0 default, 1 GitHub, 2 agent. Applies to format 0 (code requests ignore it).
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeRenderFlavor(
    mut env: JNIEnv,
    _: JClass,
    source: JString,
    format: jint,
    flavor: jint,
) -> jstring {
    render(&mut env, source, format, flavor)
}
fn render(env: &mut JNIEnv, source: JString, format: jint, flavor: jint) -> jstring {
    invoke(env, |env| {
        let flavor = u32::try_from(flavor)
            .ok()
            .and_then(markit::Flavor::from_code)
            .ok_or("Unknown flavor")?;
        let source: String = env.get_string(&source).map_err(|e| e.to_string())?.into();
        if source.len() > 16 * 1024 * 1024 {
            return Err("Document exceeds 16 MiB".into());
        }
        let document = if format == 2 || format == 3 {
            let request: serde_json::Value =
                serde_json::from_str(&source).map_err(|e| e.to_string())?;
            let (Some(source), Some(language)) =
                (request["source"].as_str(), request["language"].as_str())
            else {
                return Err("Invalid code request".into());
            };
            if format == 3 {
                markit::code_document_grammar(source, language)
                    .ok_or("Build with the grammars feature")?
            } else {
                // Byte-identical JSON streamed from compact runs.
                return Ok(markit::code_document_json(source, language));
            }
        } else if format == 0 {
            markit::parse_with(&source, flavor)
        } else {
            return Err("Unknown format".into());
        };
        serde_json::to_string(&document).map_err(|e| e.to_string())
    })
}
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeCreateStream(
    mut env: JNIEnv,
    _: JClass,
) -> jlong {
    create_stream(&mut env, 0)
}
/// `flavor`: 0 default, 1 GitHub, 2 agent.
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeCreateStreamFlavor(
    mut env: JNIEnv,
    _: JClass,
    flavor: jint,
) -> jlong {
    create_stream(&mut env, flavor)
}
fn create_stream(env: &mut JNIEnv, flavor: jint) -> jlong {
    let Some(flavor) = u32::try_from(flavor)
        .ok()
        .and_then(markit::Flavor::from_code)
    else {
        let _ = env.throw_new("java/lang/IllegalArgumentException", "Unknown flavor");
        return 0;
    };
    let Ok(mut registry) = streams().lock() else {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "Stream registry unavailable",
        );
        return 0;
    };
    if registry.len() >= 64 {
        let _ = env.throw_new(
            "java/lang/IllegalStateException",
            "Close unused Markdown streams",
        );
        return 0;
    }
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    registry.insert(
        id,
        Arc::new(Mutex::new(
            markit::StreamRenderer::new_native().with_flavor(flavor),
        )),
    );
    id
}
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeAppend(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
    delta: JString,
    finalized: jboolean,
) -> jstring {
    invoke(&mut env, |env| {
        let delta: String = env.get_string(&delta).map_err(|e| e.to_string())?.into();
        let stream = streams()
            .lock()
            .map_err(|e| e.to_string())?
            .get(&id)
            .cloned()
            .ok_or("Closed stream")?;
        let mut stream = stream.lock().map_err(|e| e.to_string())?;
        if stream.source().len() + delta.len() > 8 * 1024 * 1024 {
            return Err("Document exceeds 8 MiB".into());
        }
        serde_json::to_string(&stream.append(&delta, finalized != 0)).map_err(|e| e.to_string())
    })
}
/// MKNP v2 bytes (see docs/binary-transport.md). Throws like nativeAppend.
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeAppendPatch(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
    delta: JString,
    finalized: jboolean,
) -> jbyteArray {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || -> Result<Vec<u8>, String> {
            let delta: String = env.get_string(&delta).map_err(|e| e.to_string())?.into();
            let stream = streams()
                .lock()
                .map_err(|e| e.to_string())?
                .get(&id)
                .cloned()
                .ok_or("Closed stream")?;
            let mut stream = stream.lock().map_err(|e| e.to_string())?;
            if stream.source().len() + delta.len() > 8 * 1024 * 1024 {
                return Err("Document exceeds 8 MiB".into());
            }
            stream
                .append_native_patch(&delta, finalized != 0)
                .map_err(|e| format!("{e:?}"))
        },
    ));
    let bytes = match result {
        Ok(Ok(bytes)) => bytes,
        other => {
            let error = match other {
                Ok(Err(error)) => error,
                _ => "Markit rendering failed".into(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/IllegalStateException", error);
            }
            return ptr::null_mut();
        }
    };
    env.byte_array_from_slice(&bytes)
        .map(|array| array.into_raw())
        .unwrap_or(ptr::null_mut())
}
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeFreeStream(_: JNIEnv, _: JClass, id: jlong) {
    if let Ok(mut registry) = streams().lock() {
        registry.remove(&id);
    }
}

#[cfg(feature = "grammars")]
thread_local! {
    static LINE_HIGHLIGHTERS: RefCell<HashMap<i64, markit::LineHighlighter>> =
        RefCell::new(HashMap::new());
}

/// A line highlighter belongs to the thread that created it, so no handle
/// needs to cross threads. Ids are still unique across the process.
#[cfg(feature = "grammars")]
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeCreateLineHighlighter(
    mut env: JNIEnv,
    _: JClass,
    language: JString,
) -> jlong {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<i64, String> {
            let language: String = env.get_string(&language).map_err(|e| e.to_string())?.into();
            LINE_HIGHLIGHTERS.with(|slot| {
                let mut slot = slot.borrow_mut();
                if slot.len() >= 64 {
                    return Err("Close unused line highlighters".into());
                }
                let id = NEXT.fetch_add(1, Ordering::Relaxed);
                slot.insert(id, markit::LineHighlighter::new(&language));
                Ok(id)
            })
        }));
    match result {
        Ok(Ok(id)) => id,
        other => {
            let error = match other {
                Ok(Err(error)) => error,
                _ => "Markit highlighting failed".into(),
            };
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/IllegalStateException", error);
            }
            0
        }
    }
}

#[cfg(feature = "grammars")]
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeLineSetText(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
    text: JString,
) {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
            let text: String = env.get_string(&text).map_err(|e| e.to_string())?.into();
            LINE_HIGHLIGHTERS.with(|slot| {
                let mut slot = slot.borrow_mut();
                let highlighter = slot.get_mut(&id).ok_or("Closed line highlighter")?;
                highlighter.set_text(&text);
                Ok(())
            })
        }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        _ => "Markit highlighting failed".into(),
    };
    if !env.exception_check().unwrap_or(true) {
        let _ = env.throw_new("java/lang/IllegalStateException", error);
    }
}

/// `inserted` is a JSON array of line strings, none with a line ending.
#[cfg(feature = "grammars")]
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeLineEdit(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
    start_line: jint,
    removed_lines: jint,
    inserted: JString,
) {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
            let inserted: String = env.get_string(&inserted).map_err(|e| e.to_string())?.into();
            let lines: Vec<String> = serde_json::from_str(&inserted).map_err(|e| e.to_string())?;
            let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
            let start = usize::try_from(start_line).map_err(|_| "Invalid start line")?;
            let removed = usize::try_from(removed_lines).map_err(|_| "Invalid removed lines")?;
            LINE_HIGHLIGHTERS.with(|slot| {
                let mut slot = slot.borrow_mut();
                let highlighter = slot.get_mut(&id).ok_or("Closed line highlighter")?;
                highlighter.edit(start, removed, &lines);
                Ok(())
            })
        }));
    let error = match result {
        Ok(Ok(())) => return,
        Ok(Err(error)) => error,
        _ => "Markit highlighting failed".into(),
    };
    if !env.exception_check().unwrap_or(true) {
        let _ = env.throw_new("java/lang/IllegalStateException", error);
    }
}

#[cfg(feature = "grammars")]
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeLineTokens(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
    start_line: jint,
    end_line: jint,
) -> jstring {
    invoke(&mut env, |_| {
        let start = usize::try_from(start_line).map_err(|_| "Invalid start line")?;
        let end = usize::try_from(end_line).map_err(|_| "Invalid end line")?;
        LINE_HIGHLIGHTERS.with(|slot| {
            let mut slot = slot.borrow_mut();
            let highlighter = slot.get_mut(&id).ok_or("Closed line highlighter")?;
            serde_json::to_string(&highlighter.tokens(start, end)).map_err(|e| e.to_string())
        })
    })
}

#[cfg(feature = "grammars")]
#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeFreeLineHighlighter(
    _: JNIEnv,
    _: JClass,
    id: jlong,
) {
    LINE_HIGHLIGHTERS.with(|slot| {
        slot.borrow_mut().remove(&id);
    });
}
