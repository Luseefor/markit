use jni::{
    objects::{JClass, JString},
    sys::jstring,
    JNIEnv,
};
use std::ptr;

#[no_mangle]
pub extern "system" fn Java_dev_markit_Markit_nativeRender(
    mut env: JNIEnv,
    _class: JClass,
    source: JString,
) -> jstring {
    // Never unwind through the JVM. JNI handles Unicode using Java's UTF-16 ABI.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let source: String = env.get_string(&source)?.into();
        env.new_string(markit::render_document_json(&source))
            .map(|s| s.into_raw())
    }));
    match result {
        Ok(Ok(output)) => output,
        Ok(Err(error)) => {
            if !env.exception_check().unwrap_or(true) {
                let _ = env.throw_new("java/lang/IllegalArgumentException", error.to_string());
            }
            ptr::null_mut()
        }
        Err(_) => {
            let _ = env.throw_new("java/lang/RuntimeException", "Markit rendering failed");
            ptr::null_mut()
        }
    }
}
