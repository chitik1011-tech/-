use std::{
    collections::HashMap,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

use jni::{
    objects::{GlobalRef, JObject, JString, JValue},
    sys::{jboolean, jlong, jstring, JNI_FALSE, JNI_TRUE, JNI_VERSION_1_6},
    JNIEnv, JavaVM,
};
use tg_mobile_core::{
    backend::BackendFuture, decode_command, encode_event, encode_state, spawn_core, CoreCommand,
    CoreError, CoreEvent, CoreHandle, TelegramBackend,
};
use tokio::{runtime::Runtime, task::JoinHandle};

const CALLBACK_METHOD: &str = "onNativeEvent";
const CALLBACK_SIGNATURE: &str = "(Ljava/lang/String;Ljava/lang/String;)V";

static NEXT_HANDLE: AtomicI64 = AtomicI64::new(1);
static REGISTRY: OnceLock<Mutex<HashMap<i64, NativeHandle>>> = OnceLock::new();
static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

#[derive(Debug)]
enum BridgeError {
    Jni(String),
    InvalidHandle(i64),
    InvalidCommand(String),
    Runtime(String),
    RegistryUnavailable,
}

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Jni(message) => write!(f, "JNI error: {message}"),
            Self::InvalidHandle(handle) => write!(f, "invalid native handle: {handle}"),
            Self::InvalidCommand(message) => write!(f, "invalid command: {message}"),
            Self::Runtime(message) => write!(f, "runtime error: {message}"),
            Self::RegistryUnavailable => write!(f, "native handle registry is unavailable"),
        }
    }
}

struct NativeHandle {
    core: CoreHandle,
    callback: GlobalRef,
    vm: Arc<JavaVM>,
    event_task: JoinHandle<()>,
    state_task: JoinHandle<()>,
}

#[derive(Default)]
struct DisconnectedBackend;

impl TelegramBackend for DisconnectedBackend {
    fn handle<'a>(&'a mut self, command: CoreCommand) -> BackendFuture<'a> {
        Box::pin(async move {
            if matches!(command, CoreCommand::Shutdown) {
                return Ok(Vec::new());
            }

            Err(CoreError::Backend(
                "Telegram backend is not linked into the Android bridge yet".to_owned(),
            ))
        })
    }
}

fn registry() -> &'static Mutex<HashMap<i64, NativeHandle>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn runtime() -> Result<&'static Runtime, BridgeError> {
    match RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("tg-rust-core")
            .enable_all()
            .build()
            .map_err(|error| error.to_string())
    }) {
        Ok(runtime) => Ok(runtime),
        Err(message) => Err(BridgeError::Runtime(message.clone())),
    }
}

fn with_registry<T>(
    action: impl FnOnce(&mut HashMap<i64, NativeHandle>) -> Result<T, BridgeError>,
) -> Result<T, BridgeError> {
    let mut guard = registry()
        .lock()
        .map_err(|_| BridgeError::RegistryUnavailable)?;
    action(&mut guard)
}

fn event_kind(event: &CoreEvent) -> &'static str {
    match event {
        CoreEvent::State(_) => "account_state",
        CoreEvent::AuthorizationChanged(_) => "authorization_changed",
        CoreEvent::ChatsLoaded(_) => "chats_loaded",
        CoreEvent::MessageAdded(_) => "message_added",
        CoreEvent::MessageEdited(_) => "message_edited",
        CoreEvent::MessageDeleted { .. } => "message_deleted",
        CoreEvent::Error(_) => "error",
        CoreEvent::Stopped => "stopped",
    }
}

fn error_payload(message: &str) -> String {
    serde_json::json!({ "message": message }).to_string()
}

fn emit_callback(
    vm: &JavaVM,
    callback: &GlobalRef,
    kind: &str,
    payload: &str,
) -> Result<(), BridgeError> {
    let mut env = vm
        .attach_current_thread()
        .map_err(|error| BridgeError::Jni(error.to_string()))?;
    let kind_string = env
        .new_string(kind)
        .map_err(|error| BridgeError::Jni(error.to_string()))?;
    let payload_string = env
        .new_string(payload)
        .map_err(|error| BridgeError::Jni(error.to_string()))?;
    let kind_object = JObject::from(kind_string);
    let payload_object = JObject::from(payload_string);

    env.call_method(
        callback.as_obj(),
        CALLBACK_METHOD,
        CALLBACK_SIGNATURE,
        &[
            JValue::Object(&kind_object),
            JValue::Object(&payload_object),
        ],
    )
    .map_err(|error| BridgeError::Jni(error.to_string()))?;

    Ok(())
}

fn emit_error(vm: &JavaVM, callback: &GlobalRef, message: &str) {
    let payload = error_payload(message);
    let _ = emit_callback(vm, callback, "error", &payload);
}

fn create_native_handle(env: &mut JNIEnv<'_>, callback: JObject<'_>) -> Result<i64, BridgeError> {
    if callback.is_null() {
        return Err(BridgeError::Jni("callback must not be null".to_owned()));
    }

    let callback = env
        .new_global_ref(callback)
        .map_err(|error| BridgeError::Jni(error.to_string()))?;
    let vm = Arc::new(
        env.get_java_vm()
            .map_err(|error| BridgeError::Jni(error.to_string()))?,
    );
    let runtime = runtime()?;

    let core = {
        let _guard = runtime.enter();
        spawn_core(DisconnectedBackend)
    };

    let mut events = core.subscribe();
    let event_callback = callback.clone();
    let event_vm = Arc::clone(&vm);
    let event_task = runtime.spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    match encode_event(&event) {
                        Ok(payload) => {
                            let _ = emit_callback(
                                &event_vm,
                                &event_callback,
                                event_kind(&event),
                                &payload,
                            );
                        }
                        Err(error) => {
                            emit_error(&event_vm, &event_callback, &error.to_string());
                        }
                    }

                    if matches!(event, CoreEvent::Stopped) {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let payload = serde_json::json!({ "skipped": skipped }).to_string();
                    let _ = emit_callback(&event_vm, &event_callback, "events_lagged", &payload);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let mut state = core.subscribe_state();
    let state_callback = callback.clone();
    let state_vm = Arc::clone(&vm);
    let initial_state = encode_state(&state.borrow().clone())
        .map_err(|error| BridgeError::Runtime(error.to_string()))?;
    let state_task = runtime.spawn(async move {
        loop {
            if state.changed().await.is_err() {
                break;
            }

            let snapshot = state.borrow().clone();
            match encode_state(&snapshot) {
                Ok(payload) => {
                    let _ = emit_callback(&state_vm, &state_callback, "state", &payload);
                }
                Err(error) => {
                    emit_error(&state_vm, &state_callback, &error.to_string());
                }
            }
        }
    });

    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    with_registry(|registry| {
        registry.insert(
            handle,
            NativeHandle {
                core,
                callback: callback.clone(),
                vm: Arc::clone(&vm),
                event_task,
                state_task,
            },
        );
        Ok(())
    })?;

    if let Err(error) = emit_callback(&vm, &callback, "bridge_ready", "{\"ready\":true}") {
        let _ = destroy_native_handle(handle);
        return Err(error);
    }

    if let Err(error) = emit_callback(&vm, &callback, "state", &initial_state) {
        let _ = destroy_native_handle(handle);
        return Err(error);
    }

    Ok(handle)
}

fn destroy_native_handle(handle: i64) -> Result<(), BridgeError> {
    let native = with_registry(|registry| {
        registry
            .remove(&handle)
            .ok_or(BridgeError::InvalidHandle(handle))
    })?;

    native.event_task.abort();
    native.state_task.abort();

    let core = native.core;
    runtime()?.spawn(async move {
        let _ = core.send(CoreCommand::Shutdown).await;
    });

    Ok(())
}

fn send_command(handle: i64, command_json: &str) -> Result<(), BridgeError> {
    let command = decode_command(command_json)
        .map_err(|error| BridgeError::InvalidCommand(error.to_string()))?;

    let (core, vm, callback) = with_registry(|registry| {
        let native = registry
            .get(&handle)
            .ok_or(BridgeError::InvalidHandle(handle))?;
        Ok((
            native.core.clone(),
            Arc::clone(&native.vm),
            native.callback.clone(),
        ))
    })?;

    runtime()?.spawn(async move {
        if let Err(error) = core.send(command).await {
            emit_error(&vm, &callback, &error.to_string());
        }
    });

    Ok(())
}

fn probe_handle(handle: i64) -> Result<(), BridgeError> {
    let (vm, callback) = with_registry(|registry| {
        let native = registry
            .get(&handle)
            .ok_or(BridgeError::InvalidHandle(handle))?;
        Ok((Arc::clone(&native.vm), native.callback.clone()))
    })?;

    emit_callback(&vm, &callback, "jni_probe", "{\"round_trip\":true}")
}

fn read_java_string(env: &mut JNIEnv<'_>, value: JString<'_>) -> Result<String, BridgeError> {
    env.get_string(&value)
        .map(Into::into)
        .map_err(|error| BridgeError::Jni(error.to_string()))
}

fn throw_bridge_error(env: &mut JNIEnv<'_>, error: BridgeError) {
    let _ = env.throw_new("java/lang/IllegalStateException", error.to_string());
}

fn jni_bool(value: bool) -> jboolean {
    if value {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

#[no_mangle]
pub extern "system" fn JNI_OnLoad(_vm: JavaVM, _reserved: *mut std::ffi::c_void) -> jni::sys::jint {
    JNI_VERSION_1_6
}

#[no_mangle]
pub extern "system" fn Java_com_chitik_telegram_nativebridge_NativeBridge_coreVersion(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
) -> jstring {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let version = format!(
            "tg-mobile-core/android-bridge {}",
            env!("CARGO_PKG_VERSION")
        );
        env.new_string(version)
            .map(|value| value.into_raw())
            .map_err(|error| BridgeError::Jni(error.to_string()))
    }));

    match result {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => {
            throw_bridge_error(&mut env, error);
            std::ptr::null_mut()
        }
        Err(_) => {
            throw_bridge_error(
                &mut env,
                BridgeError::Runtime("panic caught at JNI boundary".to_owned()),
            );
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_chitik_telegram_nativebridge_NativeBridge_createHandle(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    callback: JObject<'_>,
) -> jlong {
    let result = catch_unwind(AssertUnwindSafe(|| {
        create_native_handle(&mut env, callback)
    }));

    match result {
        Ok(Ok(handle)) => handle as jlong,
        Ok(Err(error)) => {
            throw_bridge_error(&mut env, error);
            0
        }
        Err(_) => {
            throw_bridge_error(
                &mut env,
                BridgeError::Runtime("panic caught at JNI boundary".to_owned()),
            );
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_chitik_telegram_nativebridge_NativeBridge_sendCommand(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
    command_json: JString<'_>,
) -> jboolean {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let command_json = read_java_string(&mut env, command_json)?;
        send_command(handle as i64, &command_json)
    }));

    match result {
        Ok(Ok(())) => jni_bool(true),
        Ok(Err(error)) => {
            throw_bridge_error(&mut env, error);
            jni_bool(false)
        }
        Err(_) => {
            throw_bridge_error(
                &mut env,
                BridgeError::Runtime("panic caught at JNI boundary".to_owned()),
            );
            jni_bool(false)
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_chitik_telegram_nativebridge_NativeBridge_probe(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jboolean {
    let result = catch_unwind(AssertUnwindSafe(|| probe_handle(handle as i64)));

    match result {
        Ok(Ok(())) => jni_bool(true),
        Ok(Err(error)) => {
            throw_bridge_error(&mut env, error);
            jni_bool(false)
        }
        Err(_) => {
            throw_bridge_error(
                &mut env,
                BridgeError::Runtime("panic caught at JNI boundary".to_owned()),
            );
            jni_bool(false)
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_chitik_telegram_nativebridge_NativeBridge_destroyHandle(
    mut env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    let result = catch_unwind(AssertUnwindSafe(|| destroy_native_handle(handle as i64)));

    match result {
        Ok(Ok(())) => {}
        Ok(Err(BridgeError::InvalidHandle(_))) => {}
        Ok(Err(error)) => throw_bridge_error(&mut env, error),
        Err(_) => throw_bridge_error(
            &mut env,
            BridgeError::Runtime("panic caught at JNI boundary".to_owned()),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_start_command() {
        let parsed = decode_command(r#"{"type":"start"}"#);
        assert!(matches!(parsed, Ok(CoreCommand::Start)));
    }

    #[test]
    fn rejects_noncanonical_command_payload() {
        let parsed = decode_command(r#"{"type":"set_phone_number","phone_number":"123"}"#);
        assert!(parsed.is_err());
    }

    #[test]
    fn serializes_core_event_with_core_wire_format() {
        let event = CoreEvent::Error("test".to_owned());
        let encoded = encode_event(&event);
        assert!(matches!(encoded, Ok(payload) if payload.contains("\"type\":\"error\"")));
    }
}
