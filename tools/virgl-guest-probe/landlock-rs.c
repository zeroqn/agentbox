/*
 * landlock-rs: applies loftd's render-server Landlock rule set before
 * exec'ing virgl_render_server, for the bare-VM chromium probe.
 *
 * Variants:
 *   landlock-rs as-shipped <rs> --socket-fd=N   (writable rules WITHOUT Refer)
 *   landlock-rs refer-fix   <rs> --socket-fd=N   (writable rules WITH Refer
 *                                                 and the full Make set)
 *
 * Hypothesis: the render-server writable rule omits REFER, so the mesa
 * shader-cache rename under /dev/shm fails EACCES, surfacing in the guest as
 * ANGLE "Internal Vulkan error (-1): A host memory allocation has failed"
 * (VK_ERROR_OUT_OF_HOST_MEMORY) at vk_renderer.cpp initialize:2632.
 */
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <linux/landlock.h>
#include <linux/types.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>

#ifndef SYS_landlock_create_ruleset
#define SYS_landlock_create_ruleset 444
#endif
#ifndef SYS_landlock_add_rule
#define SYS_landlock_add_rule 445
#endif
#ifndef SYS_landlock_restrict_self
#define SYS_landlock_restrict_self 446
#endif

static void die(const char *what, int rc) {
    fprintf(stderr, "landlock-rs: %s failed rc=%d errno=%d (%s)\n", what, rc, errno,
            strerror(errno));
    _exit(2);
}

int main(int argc, char **argv) {
    if (argc < 4) {
        fprintf(stderr, "usage: %s <as-shipped|refer-fix> <rs-exec-path> --socket-fd=N\n",
                argv[0]);
        return 2;
    }
    const char *variant = argv[1];
    const char *rs_path = argv[2];
    const char *socket_opt = argv[3];
    int use_refer = (strcmp(variant, "refer-fix") == 0);

    __u64 handled = 0;
    handled |= LANDLOCK_ACCESS_FS_EXECUTE;
    handled |= LANDLOCK_ACCESS_FS_WRITE_FILE;
    handled |= LANDLOCK_ACCESS_FS_READ_FILE;
    handled |= LANDLOCK_ACCESS_FS_READ_DIR;
    handled |= LANDLOCK_ACCESS_FS_REMOVE_DIR;
    handled |= LANDLOCK_ACCESS_FS_REMOVE_FILE;
    handled |= LANDLOCK_ACCESS_FS_MAKE_CHAR;
    handled |= LANDLOCK_ACCESS_FS_MAKE_DIR;
    handled |= LANDLOCK_ACCESS_FS_MAKE_REG;
    handled |= LANDLOCK_ACCESS_FS_MAKE_SOCK;
    handled |= LANDLOCK_ACCESS_FS_MAKE_FIFO;
    handled |= LANDLOCK_ACCESS_FS_MAKE_BLOCK;
    handled |= LANDLOCK_ACCESS_FS_MAKE_SYM;
    handled |= LANDLOCK_ACCESS_FS_REFER;
    handled |= LANDLOCK_ACCESS_FS_TRUNCATE;
    handled |= LANDLOCK_ACCESS_FS_IOCTL_DEV;

    struct landlock_ruleset_attr attr = { .handled_access_fs = handled };
    int fd = (int)syscall(SYS_landlock_create_ruleset, &attr, sizeof(attr), 0);
    if (fd < 0) die("landlock_create_ruleset", fd);

    const __u64 read_execute = LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_READ_FILE |
                               LANDLOCK_ACCESS_FS_READ_DIR;
    const __u64 read = LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR;
    const __u64 writable = LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_WRITE_FILE |
                           LANDLOCK_ACCESS_FS_READ_DIR | LANDLOCK_ACCESS_FS_EXECUTE |
                           LANDLOCK_ACCESS_FS_TRUNCATE | LANDLOCK_ACCESS_FS_MAKE_REG |
                           LANDLOCK_ACCESS_FS_MAKE_DIR | LANDLOCK_ACCESS_FS_REMOVE_FILE |
                           LANDLOCK_ACCESS_FS_REMOVE_DIR;
    const __u64 writable_fix = writable | LANDLOCK_ACCESS_FS_MAKE_CHAR |
                               LANDLOCK_ACCESS_FS_MAKE_SOCK | LANDLOCK_ACCESS_FS_MAKE_FIFO |
                               LANDLOCK_ACCESS_FS_MAKE_BLOCK | LANDLOCK_ACCESS_FS_MAKE_SYM |
                               LANDLOCK_ACCESS_FS_REFER | LANDLOCK_ACCESS_FS_IOCTL_DEV;

    static const struct {
        const char *path;
        __u64 access;
    } rules[] = {
        { "/nix/store", read_execute },
        { "/dev", read_execute | LANDLOCK_ACCESS_FS_WRITE_FILE | LANDLOCK_ACCESS_FS_TRUNCATE |
                      LANDLOCK_ACCESS_FS_IOCTL_DEV },
        { "/sys", read },
        { "/proc", read },
        { "/dev/shm", 0 },
        { "/tmp", 0 },
    };
    const size_t n_rules = sizeof(rules) / sizeof(rules[0]);
    for (size_t i = 0; i < n_rules; i++) {
        const char *path = rules[i].path;
        __u64 access = rules[i].access;
        if (strcmp(path, "/dev/shm") == 0 || strcmp(path, "/tmp") == 0)
            access = use_refer ? writable_fix : writable;

        int pfd = open(path, O_PATH | O_CLOEXEC);
        if (pfd < 0) {
            fprintf(stderr, "landlock-rs: open(%s) failed: %s\n", path, strerror(errno));
            continue;
        }
        struct landlock_path_beneath_attr pb = { .allowed_access = access, .parent_fd = pfd };
        int rc = (int)syscall(SYS_landlock_add_rule, fd, LANDLOCK_RULE_PATH_BENEATH, &pb, 0);
        if (rc != 0) {
            fprintf(stderr, "landlock-rs: add_rule(%s) failed rc=%d errno=%d (%s)\n", path, rc,
                    errno, strerror(errno));
        }
        close(pfd);
    }

    if (prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0) die("prctl(no_new_privs)", errno);
    if (syscall(SYS_landlock_restrict_self, fd, 0) != 0) die("landlock_restrict_self", errno);
    close(fd);
    fprintf(stderr, "landlock-rs: rules applied variant=%s refer=%d\n", variant, use_refer);

    char *argv_child[] = { (char *)"virgl_render_server", (char *)socket_opt, NULL };
    execv(rs_path, argv_child);
    fprintf(stderr, "landlock-rs: execv(%s) failed errno=%d (%s)\n", rs_path, errno,
            strerror(errno));
    return 127;
}