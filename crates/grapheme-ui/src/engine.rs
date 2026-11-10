//! Engine options a host passes into [`crate::GraphemeSurface::mount`].
//!
//! The hooks are the public [`grapheme_sdk::GraphemeEngine`] builder. This
//! module does not construct a [`grapheme_runtime::StoreSession`] itself.

use std::sync::Arc;

pub use grapheme_runtime::{
    CapabilityCall, CapabilityHost, HostCallError, ModuleRegistry, PolicyGuard,
};
use grapheme_sdk::GraphemeEngine;
use serde_json::Value as JsonValue;

type HostFactory = Arc<dyn Fn() -> Box<dyn CapabilityHost + Send> + Send + Sync>;
type Interceptor =
    Arc<dyn Fn(&CapabilityCall) -> Option<Result<JsonValue, HostCallError>> + Send + Sync>;
type RegistryHook = Box<dyn FnOnce(&mut ModuleRegistry) + Send>;

/// Host hooks for the engine that compiles and drives one surface.
///
/// [`GraphemeSurface::mount`](crate::GraphemeSurface::mount) uses
/// [`SurfaceBuilder::new`]. A host that needs a policy, a host factory, an
/// interceptor, or a module-registry edit starts from
/// [`GraphemeSurface::builder`](crate::GraphemeSurface::builder).
///
/// # `const` seeds
///
/// [`grapheme_runtime::StoreSession::with_consts`] accepts host values for
/// `const` fields. [`grapheme_sdk::GraphemeEngine::store_session`] does not
/// expose that argument: it always starts from
/// [`grapheme_runtime::StoreSession::new`]. Surfacing consts needs a change
/// in the published SDK. This builder does not vendor a second session type
/// to reach the runtime constructor.
pub struct SurfaceBuilder {
    policy_guard: Option<PolicyGuard>,
    host_factory: Option<HostFactory>,
    interceptor: Option<Interceptor>,
    registry: Option<RegistryHook>,
}

impl Default for SurfaceBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SurfaceBuilder {
    /// Default engine: the SDK builder with no host hooks.
    pub fn new() -> Self {
        Self {
            policy_guard: None,
            host_factory: None,
            interceptor: None,
            registry: None,
        }
    }

    /// Admission policy for capability calls (`http`, `tcp`, `smtp`, `sql`, …).
    ///
    /// An empty allow-list on [`PolicyGuard`] does not deny HTTP. Set
    /// `allowed_http_domains` to restrict hosts.
    pub fn policy_guard(mut self, guard: PolicyGuard) -> Self {
        self.policy_guard = Some(guard);
        self
    }

    /// Replace the stdlib host. The SDK calls the factory once per invoke.
    pub fn host_factory<F>(mut self, factory: F) -> Self
    where
        F: Fn() -> Box<dyn CapabilityHost + Send> + Send + Sync + 'static,
    {
        self.host_factory = Some(Arc::new(factory));
        self
    }

    /// Override individual capability results. Return `None` to keep stdlib dispatch.
    pub fn capability_interceptor<F>(mut self, interceptor: F) -> Self
    where
        F: Fn(&CapabilityCall) -> Option<Result<JsonValue, HostCallError>> + Send + Sync + 'static,
    {
        self.interceptor = Some(Arc::new(interceptor));
        self
    }

    /// Edit the runtime module registry before the engine is built.
    pub fn configure_module_registry<F>(mut self, configure: F) -> Self
    where
        F: FnOnce(&mut ModuleRegistry) + Send + 'static,
    {
        self.registry = Some(Box::new(configure));
        self
    }

    pub(crate) fn engine(self) -> GraphemeEngine {
        let mut builder = GraphemeEngine::builder();
        if let Some(guard) = self.policy_guard {
            builder = builder.with_policy_guard(guard);
        }
        if let Some(factory) = self.host_factory {
            builder = builder.with_host_factory(move || factory());
        }
        if let Some(interceptor) = self.interceptor {
            builder = builder.with_capability_interceptor(move |call| interceptor(call));
        }
        if let Some(configure) = self.registry {
            builder = builder.configure_module_registry(configure);
        }
        builder.build()
    }
}
