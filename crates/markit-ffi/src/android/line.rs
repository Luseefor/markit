use jni::{
    objects::{JClass, JString},
    sys::{jboolean, jint, jlong, jstring},
    JNIEnv,
};
use std::{
    collections::HashMap,
    sync::{atomic::Ordering, Mutex, OnceLock},
};

use super::{invoke, NEXT};
use crate::MarkitLineHighlighter;

fn line_highlighters() -> &'static Mutex<HashMap<i64, MarkitLineHighlighter>> {
    static LINE_HIGHLIGHTERS: OnceLock<Mutex<HashMap<i64, MarkitLineHighlighter>>> =
        OnceLock::new();
    LINE_HIGHLIGHTERS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeCreateLineHighlighter(
    mut env: JNIEnv,
    _: JClass,
    language: JString,
) -> jlong {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<i64, String> {
            let language: String = env.get_string(&language).map_err(|e| e.to_string())?.into();
            let mut registry = line_highlighters().lock().map_err(|e| e.to_string())?;
            if registry.len() >= 64 {
                return Err("Close unused line highlighters".into());
            }
            let id = NEXT.fetch_add(1, Ordering::Relaxed);
            registry.insert(
                id,
                MarkitLineHighlighter {
                    inner: markit::LineHighlighter::new(&language),
                },
            );
            Ok(id)
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
            let mut registry = line_highlighters().lock().map_err(|e| e.to_string())?;
            let highlighter = registry.get_mut(&id).ok_or("Closed line highlighter")?;
            highlighter.inner.set_text(&text);
            Ok(())
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
            let mut registry = line_highlighters().lock().map_err(|e| e.to_string())?;
            let highlighter = registry.get_mut(&id).ok_or("Closed line highlighter")?;
            highlighter.inner.edit(start, removed, &lines);
            Ok(())
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

#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeLineSetBudget(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
    budget: jint,
) {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
            let budget = usize::try_from(budget).map_err(|_| "Invalid budget")?;
            let mut registry = line_highlighters().lock().map_err(|e| e.to_string())?;
            let highlighter = registry.get_mut(&id).ok_or("Closed line highlighter")?;
            highlighter.inner.set_budget(budget);
            Ok(())
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

#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeLineAdvance(
    mut env: JNIEnv,
    _: JClass,
    id: jlong,
    max_lines: jint,
) -> jboolean {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<bool, String> {
            let max_lines = usize::try_from(max_lines).map_err(|_| "Invalid line count")?;
            let mut registry = line_highlighters().lock().map_err(|e| e.to_string())?;
            let highlighter = registry.get_mut(&id).ok_or("Closed line highlighter")?;
            Ok(highlighter.inner.advance(max_lines))
        }));
    match result {
        Ok(Ok(more)) => u8::from(more),
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
        let mut registry = line_highlighters().lock().map_err(|e| e.to_string())?;
        let highlighter = registry.get_mut(&id).ok_or("Closed line highlighter")?;
        serde_json::to_string(&highlighter.inner.tokens(start, end)).map_err(|e| e.to_string())
    })
}

#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeFreeLineHighlighter(
    _: JNIEnv,
    _: JClass,
    id: jlong,
) {
    if let Ok(mut registry) = line_highlighters().lock() {
        registry.remove(&id);
    }
}
