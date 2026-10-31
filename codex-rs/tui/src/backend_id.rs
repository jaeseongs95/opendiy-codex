//! Logical backend identity, independent of a transport connection or native thread ID.
//! Reconnecting and auxiliary picker connections reuse the launch owner; a different
//! backend gets a fresh identity. Only the authenticated Desktop cron scope exposes
//! this process-local UUID; it is never a persistent or global identity.

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct BackendId(uuid::Uuid);

impl BackendId {
    #[cfg(windows)]
    pub(crate) fn desktop_cron_uuid(self) -> uuid::Uuid {
        self.0
    }

    pub(crate) fn new() -> Self {
        Self(uuid::Uuid::new_v4())
    }
}
