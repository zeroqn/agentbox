# Separate agentbox and cang image publication

Status: accepted

Agentbox and cang images publish as separate image identities while cang is incomplete. `ghcr.io/<owner>/agentbox` is built from the agentbox-compatible image output, and `ghcr.io/<owner>/cang` is built from the cang-compatible image output; each receives the release/dev mutable tag (`latest` on main push, the tag name on tag push) plus `sha-<short_sha>`.

## Context

A prior shared publication policy built one cang-compatible image and tagged it as both cang and agentbox. That kept publishing cheaper, but it made the existing agentbox fallback image depend on the still-incomplete cang image contract.

## Consequences

Existing agentbox users continue to pull an agentbox-compatible image for `latest`, `dev`, release tags, and commit SHA tags. Cang can continue publishing under its own image name without becoming the source for agentbox images. The images may still share payload packages internally until a later payload-slimming decision; the important boundary is the source image and guest-init contract used for each published name.
