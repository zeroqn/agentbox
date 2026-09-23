# Graphics and audio

Display and audio passthrough for loftd tasks. Host prerequisites for
`--gpu=drm`, `--wayland`, and `--waypipe` are listed in the
[README](../README.md#prerequisites).

## Pulse TCP audio

```ini
# ~/.config/pipewire/pipewire-pulse.conf.d/loftd-tcp.conf
pulse.properties = {
    server.address = [
        "unix:native"
        "tcp:127.0.0.1:4713"
    ]
}
```

Restart the host `pipewire-pulse` user service after adding the drop-in. Use
`--pulse=tcp:localhost:PORT` or `--pulse=tcp:127.0.0.1:PORT` to create a
private per-task bridge to the selected host IPv4-loopback listener. The guest
receives `PULSE_SERVER=unix:/run/user/<uid>/loftd-pulse`; each guest Pulse
connection crosses a dedicated libkrun vsock channel and connects only to the
configured host `127.0.0.1:PORT`. Loftd does not enable passt host-loopback
mapping or expose other host-loopback ports. Task launch succeeds if the host
listener is unavailable, and a later guest connection can succeed after the
host service starts or restarts.

Other literal IPv4 and bracketed IPv6 endpoints remain direct guest TCP
endpoints and are exported canonically as `PULSE_SERVER=tcp:IP:PORT`. Loftd
does not start host or guest `pipewire`/`pipewire-pulse`, proxy native PipeWire
`pipewire-0`, forward a Pulse cookie, or test the connection before launch.
`loftd exec` inherits the endpoint selected when the task was launched and
cannot change it.

Host `pipewire-pulse` configuration owns authorization. A permitted Pulse
client may gain playback, capture, stream inspection, or server-control access,
so configure the listener's access policy for the trust granted to the task.
For combined software-only Waypipe playback, mpv 0.41.0 also needs
`--gpu-sw=yes`, for example:

```bash
./result/bin/loftd \
  --waypipe=/tmp/loftd-waypipe.sock \
  --pulse=tcp:localhost:4713 \
  -- mpv --gpu-sw=yes '/workspace/'*.mp4
```

## Remote Waypipe

```bash
# Workstation: connect Waypipe to the local compositor.
waypipe --socket "$XDG_RUNTIME_DIR/loftd-waypipe.sock" client

# Workstation: keep an authenticated reverse Unix-socket forward open.
ssh -R /tmp/loftd-waypipe.sock:"$XDG_RUNTIME_DIR/loftd-waypipe.sock" loftd-host

# loftd host: launch a Waypipe-capable task with the initial target.
./result/bin/loftd \
  --workspace=/home/dev/foo \
  --waypipe=/tmp/loftd-waypipe.sock \
  -- gui-application

# Reuse the running Waypipe server for another GUI command.
./result/bin/loftd --waypipe exec TASK -- another-gui-application

# Replace the target, restart the guest Waypipe server, then run a command.
./result/bin/loftd --waypipe=/tmp/other-waypipe.sock exec TASK -- gui-application
```

loftd validates that the selected workspace is an absolute directory and each
valued socket path is absolute and already exists as a Unix socket. The guest
command is optional; when omitted, loftd starts the normal interactive fish
login shell. Valueless `--waypipe` launches the task capability without an
active target. A valueless Waypipe exec reuses the running server and display.
A valued Waypipe exec serially changes the target, terminates and reaps the
running server, starts a fresh server on the stable display name, waits for
readiness, and only then starts the command. This is replacement, not protocol
reconnection: existing GUI applications connected to the old server lose their
Wayland connection and normally exit. If `--workspace` is omitted, loftd uses
the current working directory. loftd does not start SSH or the workstation
Waypipe client and does not create, unlink, or clean up the forwarded socket.
Without `--gpu=drm`, the mode passes `--no-gpu` to Waypipe. The loftd guest
image provides Mesa software rendering for this path: OpenGL/EGL applications
use llvmpipe and Vulkan applications use lavapipe on the guest CPU. When
`--gpu=drm` is also selected, guest-init omits Waypipe's `--no-gpu`, does not
force the software-renderer environment, and preserves the DRM-scoped Mesa
OpenGL/EGL and Vulkan discovery paths. `--waypipe` remains mutually exclusive
with `--wayland`.
