use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use api::protocol::{Breadcrumb, Event, Exception, Level, Map, User, Value};
use scope::{with_client_and_scope, with_stack};
use utils::current_stacktrace;

// public api from other crates
pub use sentry_types::{Dsn, DsnParseError, ProjectId, ProjectIdParseError};
pub use sentry_types::protocol::v7 as protocol;

// public exports from this crate
pub use client::{Client, ClientOptions, IntoClientConfig};
pub use scope::{pop_scope, push_scope};

/// Helper struct that is returned from `init`.
///
/// When this is dropped events are drained with a 1 second timeout.
pub struct ClientInitGuard(Option<Arc<Client>>);

impl ClientInitGuard {
    /// Returns `true` if a client was created by initialization.
    pub fn is_enabled(&self) -> bool {
        self.0.is_some()
    }

    /// Returns the client created by `init`.
    pub fn client(&self) -> Option<Arc<Client>> {
        self.0.clone()
    }
}

impl Drop for ClientInitGuard {
    fn drop(&mut self) {
        if let Some(ref client) = self.0 {
            client.drain_events(Some(Duration::from_secs(2)));
        }
    }
}

/// Creates the Sentry client for a given client config and binds it.
///
/// This returns a client init guard that if kept in scope will help the
/// client send events before the application closes by calling drain on
/// the generated client.  If the scope guard is immediately dropped then
/// no draining will take place so ensure it's bound to a variable.
///
/// # Examples
///
/// ```rust
/// fn main() {
///     let _sentry = sentry::init("https://key@sentry.io/1234");
/// }
/// ```
///
/// This behaves similar to creating a client by calling `Client::from_config`
/// but gives a simplified interface that transparently handles clients not
/// being created by the Dsn being empty.
pub fn init<C: IntoClientConfig>(cfg: C) -> ClientInitGuard {
    ClientInitGuard(Client::from_config(cfg)
        .map(|client| {
            let client = Arc::new(client);
            bind_client(client.clone());
            client
        }))
}

/// Returns the currently bound client if there is one.
///
/// This might return `None` in case there is no client.  For the most part
/// code will not use this function but instead directly call `capture_event`
/// and similar functions which work on the currently active client.
pub fn current_client() -> Option<Arc<Client>> {
    with_stack(|stack| stack.client())
}

/// Rebinds the client on the current scope.
///
/// The current scope is defined as the current thread.  If a new thread spawns
/// it inherits the client of the process.  The main thread is specially handled
/// in the sense that if the main thread binds a client it becomes bound to the
/// process.
pub fn bind_client(client: Arc<Client>) {
    with_stack(|stack| stack.bind_client(client));
}

/// Captures an event on the currently active client if any.
///
/// The event must already be assembled.  Typically code would instead use
/// the utility methods like `capture_exception`.
pub fn capture_event(event: Event) -> Uuid {
    with_client_and_scope(|client, scope| client.capture_event(event, Some(scope)))
}

/// Captures an error.
///
/// This attaches the current stacktrace automatically.
pub fn capture_exception(ty: &str, value: Option<String>) -> Uuid {
    with_client_and_scope(|client, scope| {
        let event = Event {
            exceptions: vec![
                Exception {
                    ty: ty.to_string(),
                    value: value,
                    stacktrace: current_stacktrace(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        client.capture_event(event, Some(scope))
    })
}

/// Captures an arbitrary message.
pub fn capture_message(msg: &str, level: Level) -> Uuid {
    with_client_and_scope(|client, scope| {
        let event = Event {
            message: Some(msg.to_string()),
            level: level,
            ..Default::default()
        };
        client.capture_event(event, Some(scope))
    })
}

/// Records a breadcrumb by calling a function.
///
/// The total number of breadcrumbs that can be recorded are limited by the
/// configuration on the client.  This takes a callback because if the client
/// is not interested in breadcrumbs none will be recorded.
pub fn add_breadcrumb<F: FnOnce() -> Breadcrumb>(f: F) {
    with_client_and_scope(|client, scope| {
        let limit = client.options().max_breadcrumbs;
        if limit > 0 {
            scope.breadcrumbs.push_back(f());
            while scope.breadcrumbs.len() > limit {
                scope.breadcrumbs.pop_front();
            }
        }
    })
}

/// Sets the user context.
pub fn set_user_context(user: Option<User>) {
    with_client_and_scope(|_, scope| {
        scope.user = user;
    });
}

/// Sets the tags context.
pub fn set_tags_context(tags: Map<String, String>) {
    with_client_and_scope(|_, scope| {
        scope.tags = if tags.is_empty() { None } else { Some(tags) };
    });
}

/// Sets the extra context.
pub fn set_extra_context(extra: Map<String, Value>) {
    with_client_and_scope(|_, scope| {
        scope.extra = if extra.is_empty() { None } else { Some(extra) };
    });
}

/// Drain events that are not yet sent of the current client.
///
/// This calls into `drain_events` of the currently active client.  See that function
/// for more information.
pub fn drain_events(timeout: Option<Duration>) {
    with_client_and_scope(|client, _| {
        client.drain_events(timeout);
    });
}