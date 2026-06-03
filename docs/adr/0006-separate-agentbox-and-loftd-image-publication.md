# Separate agentbox and loftd image publication

Status: accepted

Agentbox and loftd images publish as separate image identities while loftd is incomplete. `ghcr.io/<owner>/agentbox` is built from the agentbox-compatible image output, and `ghcr.io/<owner>/loftd` is built from the loftd-compatible image output; each receives the release/dev mutable tag plus `sha-<short_sha>`.

## Context

A prior shared publication policy built one loftd-compatible image and tagged it as both loftd and agentbox. That kept publishing cheaper, but it made the existing agentbox fallback image depend on the still-incomplete loftd image contract.

## Consequences

Existing agentbox users continue to pull an agentbox-compatible image for `latest`, `dev`, release tags, and commit SHA tags. Loftd can continue publishing under its own image name without becoming the source for agentbox images. The images may still share payload packages internally until a later payload-slimming decision; the important boundary is the source image and guest-init contract used for each published name.
