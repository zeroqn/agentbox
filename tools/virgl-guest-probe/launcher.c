/* launcher.c
 *
 * Host-side driver for the in-guest virgl venus probe.  It boots a bare libkrun
 * microVM with a virtio-gpu device configured for the VENUS renderer (the path
 * a libkrun guest uses for Vulkan), sets the guest rootfs, routes the guest
 * console to a host file, and hands control to libkrun.
 *
 * Mirrors the product's libkrun call ordering (create_ctx -> set_vm_config ->
 * set_root -> set_gpu_options2 -> start_enter).  It intentionally does NOT use
 * krun_set_exec: the guest rootfs provides its own /init, which mounts /dev,
 * /proc, /sys, sets the Mesa venus discovery env, and runs guest-probe.
 *
 * krun_start_enter does not return on success -- it takes over the process and
 * exits with the guest's exit code.  The probe verdict is written to the guest
 * console, which with krun_set_console_output lands in a host file (run.sh
 * greps it for RESULT).
 *
 * Flag set: VENUS(1<<6) | NO_VIRGL(1<<7) | RENDER_SERVER(1<<9) = 0x2c0.  This is
 * the host-side combination the virgl-render-server-probe proved creates a
 * venus context; render-server is enabled because that is the isolated path the
 * loftd sandbox is expected to trap on, and it is honored by venus.
 */
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

#include <libkrun.h>

#define FLAGS_VENUS_RS \
    (VIRGLRENDERER_VENUS | VIRGLRENDERER_NO_VIRGL | VIRGLRENDERER_RENDER_SERVER)

static void fail(const char *what, int rc) {
    fprintf(stderr, "launcher: %s failed rc=%d\n", what, rc);
    exit(1);
}

/* argv[1] = path to the guest rootfs directory (built by guest-rootfs.nix)
 * argv[2] = path to the host file that should receive the guest console output */
int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr,
                "usage: %s <rootfs-dir> <console-output-file> [gpu-flags-hex]\n",
                argv[0]);
        return 2;
    }
    const char *rootfs = argv[1];
    const char *console = argv[2];
    uint32_t gpu_flags = FLAGS_VENUS_RS;
    if (argc >= 4)
        gpu_flags = (uint32_t)strtoul(argv[3], NULL, 0);

    krun_init_log(KRUN_LOG_TARGET_DEFAULT, KRUN_LOG_LEVEL_WARN, KRUN_LOG_STYLE_NEVER, 0);

    int32_t ctx = krun_create_ctx();
    if (ctx < 0)
        fail("krun_create_ctx", ctx);

    int32_t rc;
    if ((rc = krun_set_vm_config((uint32_t)ctx, 2, 512)) < 0)
        fail("krun_set_vm_config", rc);

    if ((rc = krun_set_root((uint32_t)ctx, rootfs)) < 0)
        fail("krun_set_root", rc);

    /* venus virtio-gpu with a 256 MiB SHM window -- same size the product uses. */
    if ((rc = krun_set_gpu_options2((uint32_t)ctx, gpu_flags,
                                    256ull * 1024 * 1024)) < 0)
        fail("krun_set_gpu_options2", rc);

    /* Route the implicit guest console to a host file so the probe verdict can
     * be read back after the VM shuts down. */
    if ((rc = krun_set_console_output((uint32_t)ctx, console)) < 0)
        fail("krun_set_console_output", rc);

    fprintf(stderr, "launcher: booting guest rootfs=%s gpu_flags=0x%x console=%s\n",
            rootfs, gpu_flags, console);

    int32_t rc2 = krun_start_enter((uint32_t)ctx);
    /* On success the VM takes over this process and exits; reaching here means
     * the microVM failed to start. */
    fail("krun_start_enter", rc2);
    return 1;
}