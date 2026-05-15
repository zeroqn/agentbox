use super::*;
use anyhow::{Result, anyhow};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, PartialEq, Eq)]
enum ProbeCall {
    LookupLabel(String),
    CandidateLabel(String),
    EnumerateCandidates(String),
}

#[derive(Default)]
struct FakeDiskProbe {
    label_lookup: Option<Result<Option<String>, String>>,
    candidate_labels: HashMap<String, Result<Option<String>, String>>,
    enumerations: HashMap<String, Vec<String>>,
    calls: RefCell<Vec<ProbeCall>>,
}

impl FakeDiskProbe {
    fn with_label_lookup(mut self, result: Result<Option<&str>, &str>) -> Self {
        self.label_lookup = Some(
            result
                .map(|path| path.map(str::to_owned))
                .map_err(str::to_owned),
        );
        self
    }

    fn with_candidate_label(mut self, candidate: &str, result: Result<Option<&str>, &str>) -> Self {
        self.candidate_labels.insert(
            candidate.to_owned(),
            result
                .map(|label| label.map(str::to_owned))
                .map_err(str::to_owned),
        );
        self
    }

    fn with_enumeration(mut self, pattern: &str, candidates: &[&str]) -> Self {
        self.enumerations.insert(
            pattern.to_owned(),
            candidates
                .iter()
                .map(|candidate| (*candidate).to_owned())
                .collect(),
        );
        self
    }

    fn calls(&self) -> Vec<ProbeCall> {
        self.calls.borrow_mut().drain(..).collect()
    }
}

impl DiskProbe for FakeDiskProbe {
    fn lookup_label(&self, label: &str) -> Result<Option<String>> {
        self.calls
            .borrow_mut()
            .push(ProbeCall::LookupLabel(label.to_owned()));
        self.label_lookup
            .clone()
            .unwrap_or(Ok(None))
            .map_err(|message| anyhow!(message))
    }

    fn candidate_label(&self, candidate: &str) -> Result<Option<String>> {
        self.calls
            .borrow_mut()
            .push(ProbeCall::CandidateLabel(candidate.to_owned()));
        self.candidate_labels
            .get(candidate)
            .cloned()
            .unwrap_or(Ok(None))
            .map_err(|message| anyhow!(message))
    }

    fn enumerate_candidates(&self, pattern: &str) -> Result<Vec<String>> {
        self.calls
            .borrow_mut()
            .push(ProbeCall::EnumerateCandidates(pattern.to_owned()));
        Ok(self.enumerations.get(pattern).cloned().unwrap_or_default())
    }
}

#[test]
fn preferred_candidate_match_returns_without_generic_discovery() {
    let probe = FakeDiskProbe::default().with_candidate_label("/dev/vda", Ok(Some("AGENTBOX_NIX")));

    let path = find_labeled_disk_with_probe("AGENTBOX_NIX", "agentbox-nix", &["/dev/vda"], &probe)
        .expect("preferred disk should be returned");

    assert_eq!(path, PathBuf::from("/dev/vda"));
    assert_eq!(
        probe.calls(),
        vec![ProbeCall::CandidateLabel("/dev/vda".to_owned())]
    );
}

#[test]
fn preferred_candidate_mismatch_falls_back_to_blkid_label_lookup() {
    let probe = FakeDiskProbe::default()
        .with_candidate_label("/dev/vda", Ok(Some("OTHER_LABEL")))
        .with_label_lookup(Ok(Some("/dev/disk/by-label/AGENTBOX_NIX")));

    let path = find_labeled_disk_with_probe("AGENTBOX_NIX", "agentbox-nix", &["/dev/vda"], &probe)
        .expect("label lookup fallback should be returned");

    assert_eq!(path, PathBuf::from("/dev/disk/by-label/AGENTBOX_NIX"));
    assert_eq!(
        probe.calls(),
        vec![
            ProbeCall::CandidateLabel("/dev/vda".to_owned()),
            ProbeCall::LookupLabel("AGENTBOX_NIX".to_owned())
        ]
    );
}

#[test]
fn preferred_candidate_without_label_falls_back_to_enumeration() {
    let probe = FakeDiskProbe::default()
        .with_candidate_label("/dev/vda", Ok(None))
        .with_label_lookup(Ok(None))
        .with_enumeration("/dev/disk/by-id/*agentbox-nix*", &[])
        .with_enumeration("/dev/vd?", &["/dev/vdb", "/dev/vdc"])
        .with_candidate_label("/dev/vdb", Ok(Some("OTHER_LABEL")))
        .with_candidate_label("/dev/vdc", Ok(Some("AGENTBOX_NIX")));

    let path = find_labeled_disk_with_probe("AGENTBOX_NIX", "agentbox-nix", &["/dev/vda"], &probe)
        .expect("enumerated fallback disk should be returned");

    assert_eq!(path, PathBuf::from("/dev/vdc"));
    assert_eq!(
        probe.calls(),
        vec![
            ProbeCall::CandidateLabel("/dev/vda".to_owned()),
            ProbeCall::LookupLabel("AGENTBOX_NIX".to_owned()),
            ProbeCall::EnumerateCandidates("/dev/disk/by-id/*agentbox-nix*".to_owned()),
            ProbeCall::EnumerateCandidates("/dev/vd?".to_owned()),
            ProbeCall::CandidateLabel("/dev/vdb".to_owned()),
            ProbeCall::CandidateLabel("/dev/vdc".to_owned())
        ]
    );
}

#[test]
fn preferred_candidate_blkid_execution_error_is_not_silently_ignored() {
    let probe =
        FakeDiskProbe::default().with_candidate_label("/dev/vda", Err("failed to run blkid"));

    let error = find_labeled_disk_with_probe("AGENTBOX_NIX", "agentbox-nix", &["/dev/vda"], &probe)
        .expect_err("blkid execution errors should stop discovery");

    assert!(error.to_string().contains("failed to run blkid"));
    assert_eq!(
        probe.calls(),
        vec![ProbeCall::CandidateLabel("/dev/vda".to_owned())]
    );
}

#[test]
fn compatibility_entrypoint_starts_with_blkid_label_lookup_when_no_preferred_candidates() {
    let probe =
        FakeDiskProbe::default().with_label_lookup(Ok(Some("/dev/disk/by-label/AGENTBOX_NIX")));

    let path = find_labeled_disk_with_probe("AGENTBOX_NIX", "agentbox-nix", &[], &probe)
        .expect("label lookup should be returned");

    assert_eq!(path, PathBuf::from("/dev/disk/by-label/AGENTBOX_NIX"));
    assert_eq!(
        probe.calls(),
        vec![ProbeCall::LookupLabel("AGENTBOX_NIX".to_owned())]
    );
}
