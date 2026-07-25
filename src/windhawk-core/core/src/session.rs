//! The session: the ownership root of all core state. Config is resolved once
//! at creation and immutable; the in-memory members are coordination only
//! (locks, operation handles, the callback queue) and never cache durable data.

use std::sync::{Arc, RwLock};

use serde_json::Value;
use windhawk_core_domain::CompileArch;
use windhawk_core_ports::{
    Clock, Files, Http, InstallerLanguage, NamedLock, Processes, StorageProvider,
};
use windhawk_core_protocol::{RequestEnvelope, response_err, response_ok};

use crate::callbacks::{CallbackDispatcher, HostCallbacks, LogLevel};
use crate::config::SessionConfig;
use crate::dispatch::{CommandKind, Handler, LockSpec, find_command};
use crate::error::CoreError;
use crate::gate::ShutdownGate;
use crate::locks::ResourceLocks;
use crate::pending::PendingArtifacts;
use crate::runtime::{OperationRegistry, PreparedOp};
use crate::services::{ProfileState, Storage};

/// The port bundle wired in by the composition root (the FFI crate in
/// production, in-memory fakes in tests). One field per external port the
/// session depends on.
pub struct Deps {
    pub clock: Arc<dyn Clock>,
    pub processes: Arc<dyn Processes>,
    pub storage: Arc<dyn StorageProvider>,
    /// The installer-language registry write (`applyAppSettings`,
    /// non-portable), split off `StorageProvider`. Best effort; a failure is
    /// logged and never fails the command.
    pub installer_language: Arc<dyn InstallerLanguage>,
    /// Filesystem access for mod sources and the user profile.
    pub files: Arc<dyn Files>,
    /// The cross-process profile read-modify-write mutex.
    pub named_lock: Arc<dyn NamedLock>,
    /// Streaming HTTP for the repository client and update download.
    pub http: Arc<dyn Http>,
}

pub struct SessionInner {
    config: SessionConfig,
    /// The compile-arch scope, resolved once at creation: the config `--arch`
    /// override, or the detected OS native machine (arm64 -> `Arm64`, else
    /// `X64`) when the config leaves it on `auto`. Selects the per-mod compile
    /// target set and, through `arm64_enabled`, the cleanup/download subfolders
    /// and the `getCoreInfo` report.
    arch: CompileArch,
    storage: Storage,
    deps: Deps,
    locks: ResourceLocks,
    /// Profile read-modify-write coordination and last-own-write mtime; holds
    /// no durable data.
    profile_state: ProfileState,
    /// DLLs written by in-flight compile/install operations, excluded from
    /// concurrent old-DLL cleanup.
    pending: Arc<PendingArtifacts>,
    ops: OperationRegistry,
    dispatcher: Arc<CallbackDispatcher>,
    gate: ShutdownGate,
}

impl SessionInner {
    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// The compile-arch scope resolved at session creation (the `--arch`
    /// override or the detected native machine). The compile orchestrator reads
    /// it to select the target set and gate the arm64-machine common-process x64
    /// skip.
    pub fn compile_arch(&self) -> CompileArch {
        self.arch
    }

    /// ARM64 eligibility resolved at session creation (aarch64 is a target world
    /// under `arm64`/`all`). Gates the aarch64 compile target (the
    /// cleanup/download subfolders and the DLL-collision sweep) and is reported
    /// by `getCoreInfo`.
    pub fn arm64_enabled(&self) -> bool {
        self.arch.arm64_enabled()
    }

    pub fn deps(&self) -> &Deps {
        &self.deps
    }

    pub fn storage(&self) -> &Storage {
        &self.storage
    }

    pub fn profile_state(&self) -> &ProfileState {
        &self.profile_state
    }

    /// The pending-artifact set: in-flight operations register their
    /// not-yet-committed DLLs here so concurrent cleanup skips them.
    pub fn pending(&self) -> Arc<PendingArtifacts> {
        self.pending.clone()
    }

    /// The keyed command `Mod` lock for `mod_id`, handed to a staged async
    /// command's body for its commit section - services take the handle from
    /// the session, they do not construct command locks.
    pub fn mod_lock(&self, mod_id: &str) -> Arc<RwLock<()>> {
        self.locks.mod_lock(mod_id)
    }

    /// The single `AppSettings` command lock. `importUserData` drives
    /// `app_settings::apply` directly (not through dispatch, which normally
    /// resolves this lock), so it takes the exclusive side around that write
    /// itself.
    pub fn app_settings_lock(&self) -> &RwLock<()> {
        self.locks.app_settings()
    }

    /// Services log through this: it enqueues to the callback dispatcher, so
    /// log callbacks fire on the dispatcher thread, never on the thread inside
    /// an invoke.
    pub fn log(&self, level: LogLevel, message: impl Into<String>) {
        self.dispatcher.log(level, message);
    }
}

pub struct Session {
    inner: Arc<SessionInner>,
}

impl Session {
    /// Create a session from the `WhCoreSessionCreate` config document: parse
    /// it, then resolve `windhawk.ini` `[Storage]` under `appRootPath` through
    /// the storage provider. A missing or invalid app root fails with
    /// `APP_ROOT_INVALID`.
    ///
    /// `detected_arm64` is the OS native-machine detection the composition root
    /// performs (`windhawk_core_windows::is_arm64_native_machine`); it resolves
    /// the compile-arch scope (`Arm64` when true, else `X64`) unless the config
    /// carries an explicit `--arch` override (`x64`/`arm64`/`all`, from tests or
    /// the CLI flag).
    pub fn create(
        config_json: &str,
        detected_arm64: bool,
        callbacks: HostCallbacks,
        deps: Deps,
    ) -> Result<Session, CoreError> {
        let config: SessionConfig = serde_json::from_str(config_json)
            .map_err(|e| CoreError::invalid_request(format!("invalid session config: {e}")))?;

        let arch = config.compile_arch_override.unwrap_or(if detected_arm64 {
            CompileArch::Arm64
        } else {
            CompileArch::X64
        });

        let resolved = deps
            .storage
            .resolve(&config.app_root_path)
            .map_err(|e| CoreError::app_root_invalid(e.message, config.app_root_path.clone()))?;
        let storage = Storage::new(resolved.info, resolved.backend);

        let dispatcher = Arc::new(CallbackDispatcher::new(callbacks));
        Ok(Session {
            inner: Arc::new(SessionInner {
                config,
                arch,
                storage,
                deps,
                locks: ResourceLocks::new(),
                profile_state: ProfileState::new(),
                pending: Arc::new(PendingArtifacts::new()),
                ops: OperationRegistry::new(),
                dispatcher,
                gate: ShutdownGate::new(),
            }),
        })
    }

    /// Synchronous command dispatch. Always returns a response envelope, never
    /// panics through.
    pub fn invoke(&self, request_json: &str) -> String {
        let Some(_guard) = self.inner.gate.enter() else {
            return response_err(&CoreError::internal("session is shutting down").to_wire());
        };

        let result = self.dispatch_sync(request_json);
        match result {
            Ok(value) => response_ok(&value),
            Err(e) => response_err(&e.to_wire()),
        }
    }

    fn dispatch_sync(&self, request_json: &str) -> Result<Value, CoreError> {
        let request: RequestEnvelope = serde_json::from_str(request_json)
            .map_err(|e| CoreError::invalid_request(format!("invalid request JSON: {e}")))?;
        let spec = find_command(&request.command).ok_or_else(|| {
            CoreError::invalid_request(format!("unknown command: {}", request.command))
        })?;
        match (&spec.kind, &spec.handler) {
            (CommandKind::Sync, Handler::Sync(run)) => {
                self.run_sync_locked(spec.locks, request.params, *run)
            }
            // Stateless commands read no session state and are
            // `LockSpec::None`, so they short-circuit before `run_sync_locked`.
            // `WhCoreInvoke` MUST keep serving them: the extension routes
            // `parseModSource` here, and the bridge holds no per-command
            // knowledge to re-route.
            (CommandKind::Sync, Handler::Stateless(run)) => run(request.params),
            _ => Err(CoreError::invalid_request(format!(
                "command is asynchronous and must be invoked via WhCoreInvokeAsync: {}",
                request.command
            ))),
        }
    }

    /// Acquire the command's declared command-level lock around the synchronous
    /// handler. Reads take the shared side, writes the exclusive side; `Mod`
    /// locks key on `params.modId` (an invalid/absent id takes no lock and lets
    /// the handler reject the params). The guard and any keyed-lock `Arc` live
    /// for the handler call.
    fn run_sync_locked(
        &self,
        locks: LockSpec,
        params: Value,
        run: fn(&SessionInner, Value) -> Result<Value, CoreError>,
    ) -> Result<Value, CoreError> {
        let inner = &self.inner;
        match locks {
            LockSpec::None => run(inner, params),
            LockSpec::AppSettings { write: true } => {
                let _g = inner
                    .locks
                    .app_settings()
                    .write()
                    .unwrap_or_else(|e| e.into_inner());
                run(inner, params)
            }
            LockSpec::AppSettings { write: false } => {
                let _g = inner
                    .locks
                    .app_settings()
                    .read()
                    .unwrap_or_else(|e| e.into_inner());
                run(inner, params)
            }
            LockSpec::Mod { write } => {
                let key = params
                    .get("modId")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                match key {
                    Some(key) => {
                        let lock = inner.locks.mod_lock(&key);
                        if write {
                            let _g = lock.write().unwrap_or_else(|e| e.into_inner());
                            run(inner, params)
                        } else {
                            let _g = lock.read().unwrap_or_else(|e| e.into_inner());
                            run(inner, params)
                        }
                    }
                    None => run(inner, params),
                }
            }
            // `Update` and `ModStaged` are async-only locks (startUpdate /
            // compileInstalledMod); no sync command declares them, so these
            // arms are unreachable in practice and take no lock.
            LockSpec::Update | LockSpec::ModStaged => run(inner, params),
        }
    }

    /// Asynchronous command start (`WhCoreInvokeAsync`): parse and
    /// validate synchronously; failures are returned without an operation
    /// id and no events are emitted. On success the operation id is
    /// nonzero and the event stream terminates with exactly one
    /// completed/failed event.
    pub fn invoke_async(&self, request_json: &str) -> Result<u64, CoreError> {
        let Some(_guard) = self.inner.gate.enter() else {
            return Err(CoreError::internal("session is shutting down"));
        };

        let request: RequestEnvelope = serde_json::from_str(request_json)
            .map_err(|e| CoreError::invalid_request(format!("invalid request JSON: {e}")))?;
        let spec = find_command(&request.command).ok_or_else(|| {
            CoreError::invalid_request(format!("unknown command: {}", request.command))
        })?;
        let prepare = match (&spec.kind, &spec.handler) {
            (CommandKind::Async, Handler::Async(prepare)) => prepare,
            _ => {
                return Err(CoreError::invalid_request(format!(
                    "command is synchronous and must be invoked via WhCoreInvoke: {}",
                    request.command
                )));
            }
        };
        // Validate params first (a malformed request returns INVALID_REQUEST
        // before any lock is taken), then acquire the command's async lock.
        let prepared = prepare(&self.inner, request.params)?;
        let prepared = self.apply_async_lock(spec.locks, prepared)?;
        Ok(self
            .inner
            .ops
            .spawn(self.inner.dispatcher.clone(), prepared))
    }

    /// Acquire an async command's declared command lock and bind it to the
    /// operation body so it is held for the operation's whole life and released
    /// when the body finishes (or is dropped unrun). Only `Update` is an async
    /// lock today: a try-acquire that fails fast with `UPDATE_IN_PROGRESS`,
    /// returned synchronously without an operation id, per the ABI. `ModStaged`
    /// (`compileInstalledMod`) acquires no lock here: the staged keyed-`Mod`
    /// lock is taken inside the operation body, around its commit section only,
    /// so the slow compile runs unlocked.
    fn apply_async_lock(
        &self,
        locks: LockSpec,
        prepared: PreparedOp,
    ) -> Result<PreparedOp, CoreError> {
        match locks {
            LockSpec::Update => {
                let Some(guard) = self.inner.locks.try_acquire_update() else {
                    return Err(CoreError::update_in_progress());
                };
                let body = prepared.0;
                Ok(PreparedOp(Box::new(move |ctx| {
                    let _guard = guard; // released when the body returns or is dropped
                    body(ctx)
                })))
            }
            _ => Ok(prepared),
        }
    }

    /// `WhCoreCancel`: cooperative, idempotent; unknown or terminal ids
    /// are a harmless no-op (returns false).
    pub fn cancel(&self, op_id: u64) -> bool {
        self.inner.ops.cancel(op_id)
    }

    /// `WhCoreSessionDestroy` semantics: drain in-flight calls, cancel and join
    /// every operation, then drain and stop the callback dispatcher. No
    /// callback fires after this returns.
    pub fn shutdown(&self) {
        self.inner.gate.close_and_wait();
        self.inner.ops.cancel_all_and_join();
        self.inner.dispatcher.shutdown();
    }
}
