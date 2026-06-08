//! Host-overlay Nix guest environment contribution.
//!
//! Normal loftd launches no longer prepare or attach a raw `/nix` disk. The
//! historical file name is retained only so tests can prove the normal path does
//! not create it.

#[cfg(test)]
pub(super) const FILE_NAME: &str = "loftd-nix.raw";

pub(super) fn host_overlay_env_pairs() -> [(String, String); 2] {
    [
        ("LOFTD_NIX_OVERLAY".to_owned(), "1".to_owned()),
        ("LOFTD_NIX_HOST_OVERLAY".to_owned(), "1".to_owned()),
    ]
}
