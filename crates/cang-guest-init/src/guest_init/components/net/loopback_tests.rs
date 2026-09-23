use super::*;

#[derive(Default)]
struct RecordingLoopbackConfigurator {
    calls: Vec<LoopbackOperation>,
}

impl LoopbackConfigurator for RecordingLoopbackConfigurator {
    fn add_ipv4_address(
        &mut self,
        interface: &str,
        address: &str,
        prefix_len: u8,
    ) -> anyhow::Result<()> {
        self.calls.push(LoopbackOperation::AddIpv4Address {
            interface: interface.to_owned(),
            address: address.to_owned(),
            prefix_len,
        });
        Ok(())
    }

    fn set_link_up(&mut self, interface: &str) -> anyhow::Result<()> {
        self.calls.push(LoopbackOperation::SetLinkUp {
            interface: interface.to_owned(),
        });
        Ok(())
    }
}

#[derive(Debug, PartialEq, Eq)]
enum LoopbackOperation {
    AddIpv4Address {
        interface: String,
        address: String,
        prefix_len: u8,
    },
    SetLinkUp {
        interface: String,
    },
}

#[test]
fn ensure_loopback_ipv4_assigns_ipv4_loopback_and_brings_link_up() {
    let mut configurator = RecordingLoopbackConfigurator::default();

    ensure_loopback_ipv4_with(&mut configurator).expect("loopback setup should succeed");

    assert_eq!(
        configurator.calls,
        [
            LoopbackOperation::AddIpv4Address {
                interface: "lo".to_owned(),
                address: "127.0.0.1".to_owned(),
                prefix_len: 8,
            },
            LoopbackOperation::SetLinkUp {
                interface: "lo".to_owned(),
            },
        ]
    );
}
