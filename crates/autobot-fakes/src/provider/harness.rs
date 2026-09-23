//! The harness through which the provider contract suite drives a fake provider.

use super::{Call, FakeProvider, Fault, ProviderFixture, Trigger};
use autobot_adapters::provider::{OperationKey, ProviderFault, ProviderHarness, TransportFault};
use autobot_adapters::text::OperationName;

/// Runs [`autobot_adapters::provider::run`] against the fake providers one fixture builds.
///
/// A fault the suite asks for is put ahead of the fixture's script: a dropped request ends in
/// [`TransportFault::Timeout`] and a lost acknowledgement in [`TransportFault::Disconnect`].
#[derive(Debug, Clone)]
pub struct FakeProviderHarness {
    fixture: ProviderFixture,
}

impl FakeProviderHarness {
    /// A harness over `fixture`.
    #[must_use]
    pub fn new(fixture: ProviderFixture) -> Self {
        Self { fixture }
    }
}

impl ProviderHarness for FakeProviderHarness {
    type Adapter = FakeProvider;

    fn adapter(&mut self) -> FakeProvider {
        self.fixture.build()
    }

    fn fault_next_send(&mut self, adapter: &mut FakeProvider, fault: ProviderFault) {
        let fault = match fault {
            ProviderFault::DroppedRequest => Fault::Dropped(TransportFault::Timeout),
            ProviderFault::LostAcknowledgement => {
                Fault::LostAcknowledgement(TransportFault::Disconnect)
            }
        };
        adapter
            .script_mut()
            .push_front(Trigger::any(Call::Send), fault);
    }

    fn applications(
        &self,
        adapter: &FakeProvider,
        operation: &OperationName,
        key: &OperationKey,
    ) -> u32 {
        adapter.applications(operation, key)
    }
}
