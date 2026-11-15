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
use serde::{Deserialize, Serialize};
use tg_mobile_core::{
    backend::BackendFuture, spawn_core, AuthorizationState, CoreCommand, CoreError, CoreEvent,
    CoreHandle, TelegramBackend,
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

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BridgeCommand {
    Start,
    SetPhoneNumber { phone_number: String },
    SubmitCode { code: String },
    SubmitPassword { password: String },
    LoadChats { limit: usize },
    SendText { chat_id: i64, text: String },
    Shutdown,
}

impl TryFrom<BridgeCommand> for CoreCommand {
    type Error = BridgeError;

    fn try_from(value: BridgeCommand) -> Result<Self, Self::Error> {
        match value {
            BridgeCommand::Start => Ok(CoreCommand::Start),
            BridgeCommand::SetPhoneNumber { phone_number } => {
                let phone_number = require_text("phone_number", phone_number)?;
                Ok(CoreCommand::SetPhoneNumber(phone_number))
            }
            BridgeCommand::SubmitCode { code } => {
                let code = require_text("code", code)?;
                Ok(CoreCommand::SubmitCode(code))
            }
            BridgeCommand::SubmitPassword { password } => {
                let password = require_text("password", password)?;
                Ok(CoreCommand::SubmitPassword(password))
            }
            BridgeCommand::LoadChats { limit } => {
                if !(1..=500).contains(&limit) {
                    return Err(BridgeError::InvalidCommand(
                        "limit must be in the range 1..=500".to_owned(),
                    ));
                }
                Ok(CoreCommand::LoadChats { limit })
            }
            BridgeCommand::SendText { chat_id, text } => {
                if chat_id == 0 {
                    return Err(BridgeError::InvalidCommand(
                        "chat_id must be non-zero".to_owned(),
                    ));
                }
                let text = require_text("text", text)?;
                Ok(CoreCommand::SendText { chat_id, text })
            }
            BridgeCommand::Shutdown => Ok(CoreCommand::Shutdown),
        }
    }
}

#[derive(Serialize)]
struct ErrorPayload<'a> {
    message: &'a str,
}

fn require_text(field: &str, value: String) -> Result<String, BridgeError> {
    if value.trim().is_empty() {
        return Err(BridgeError::InvalidCommand(format!(
            "{field} must not be empty"
        )));
    }
    Ok(value)
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

fn parse_command(json: &str) -> Result<CoreCommand, BridgeError> {
    let bridge_command: BridgeCommand = serde_json::from_str(json)
        .map_err(|error| BridgeError::InvalidCommand(error.to_string()))?;
    bridge_command.try_into()
}

fn core_event_to_payload(event: &CoreEvent) -> Result<(&'static str, String), BridgeError> {
    let result = match event {
        CoreEvent::State(state) => (
            "state",
            serde_json::to_string(state).map_err(|error| BridgeError::Runtime(error.to_string()))?,
        ),
        CoreEvent::AuthorizationChanged(authorization) => (
            "authorization_changed",
            serde_json::to_string(authorization)
                .map_err(|error| BridgeError::Runtime(error.to_string()))?,
        ),
        CoreEvent::ChatsLoaded(chats) => (
            "chats_loaded",
            serde_json::to_string(chats).map_err(|error| BridgeError::Runtime(error.to_string()))?,
        ),
        CoreEvent::MessageAdded(message) => (
            "message_added",
            serde_json::to_string(message)
                .map_err(|error| BridgeError::Runtime(error.to_string()))?,
        ),
        CoreEvent::Error(message) => (
            "error",
            serde_json::to_string(&ErrorPayload { message })
                .map_err(|error| BridgeError::Runtime(error.to_string()))?,
        ),
        CoreEvent::Stopped => ("stopped", "{}".to_owned()),
    };

    Ok(result)
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
                    if let Ok((kind, payload)) = core_event_to_payload(&event) {
                        let _ = emit_callback(&event_vm, &event_callback, kind, &payload);
                    }
                    if matches!(event, CoreEvent::Stopped) {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    let payload = format!("{{\"skipped\":{skipped}}}");
                    let _ = emit_callback(&event_vm, &event_callback, "events_lagged", &payload);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
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
            },
        );
        Ok(())
    })?;

    emit_callback(&vm, &callback, "bridge_ready", "{\"ready\":true}")?;
    Ok(handle)
}

fn destroy_native_handle(handle: i64) -> Result<(), BridgeError> {
    let native = with_registry(|registry| {
        registry
            .remove(&handle)
            .ok_or(BridgeError::InvalidHandle(handle))
    })?;

    native.event_task.abort();
    let core = native.core;
    runtime()?.spawn(async move {
        let _ = core.send(CoreCommand::Shutdown).await;
    });
    Ok(())
}

fn send_command(handle: i64, command: CoreCommand) -> Result<(), BridgeError> {
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
            let message = error.to_string();
            let payload = serde_json::to_string(&ErrorPayload { message: &message });
            if let Ok(payload) = payload {
                let _ = emit_callback(&vm, &callback, "error", &payload);
            }
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
    if value { JNI_TRUE } else { JNI_FALSE }
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
        let version = format!("tg-mobile-core/android-bridge {}", env!("CARGO_PKG_VERSION"));
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
            throw_bridge_error(&mut env, BridgeError::Runtime("panic caught at JNI boundary".to_owned()));
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
    let result = catch_unwind(AssertUnwindSafe(|| create_native_handle(&mut env, callback)));
    match result {
        Ok(Ok(handle)) => handle as jlong,
        Ok(Err(error)) => { throw_bridge_error(&mut env, error); 0 }
        Err(_) => {
            throw_bridge_error(&mut env, BridgeError::Runtime("panic caught at JNI boundary".to_owned()));
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
        let command = parse_command(&command_json)?;
        send_command(handle as i64, command)
    }));

    match result {
        Ok(Ok(())) => jni_bool(true),
        Ok(Err(error)) => { throw_bridge_error(&mut env, error); jni_bool(false) }
        Err(_) => {
            throw_bridge_error(&mut env, BridgeError::Runtime("panic caught at JNI boundary".to_owned()));
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
        Ok(Err(error)) => { throw_bridge_error(&mut env, error); jni_bool(false) }
        Err(_) => {
            throw_bridge_error(&mut env, BridgeError::Runtime("panic caught at JNI boundary".to_owned()));
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
        Err(_) => throw_bridge_error(&mut env, BridgeError::Runtime("panic caught at JNI boundary".to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_start_command() {
        let parsed = parse_command(r#"{"type":"start"}"#);
        assert!(matches!(parsed, Ok(CoreCommand::Start)));
    }

    #[test]
    fn rejects_empty_phone_number() {
        let parsed = parse_command(r#"{"type":"set_phone_number","phone_number":"  "}"#);
        assert!(matches!(parsed, Err(BridgeError::InvalidCommand(_))));
    }

    #[test]
    fn serializes_authorization_event() {
        let event = CoreEvent::AuthorizationChanged(AuthorizationState::Ready);
        let payload = core_event_to_payload(&event);
        assert!(matches!(payload, Ok(("authorization_changed", _))));
    }
}
