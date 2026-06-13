pub(crate) mod admin;
pub(crate) mod buttons;
pub(crate) mod connect;
pub(crate) mod logs;
pub(crate) mod movement;
pub(crate) mod query;

use std::sync::Arc;

use crate::link::Link;
use crate::transport::Transport;
use crate::types::CountersSnapshot;

/// The host control handle for one medius box.
#[derive(Clone, Debug)]
pub struct Device {
    pub(crate) link: Link,
}

impl Device {
    pub(crate) fn from_transport(transport: Arc<dyn Transport>) -> Device {
        Device {
            link: Link::from_transport(transport),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn from_transport_with_cadence(
        transport: Arc<dyn Transport>,
        keepalive_cadence: std::time::Duration,
    ) -> Device {
        Device {
            link: Link::from_transport_with_cadence(transport, keepalive_cadence),
        }
    }

    /// A snapshot of the always-on counters.
    pub fn counters(&self) -> CountersSnapshot {
        self.link.counters()
    }
}
