/* sandbox-rs.c
 *
 * Replicates loftd's render-server seccomp sandbox applied BEFORE exec of
 * virgl_render_server (mirror of render_server.rs: no_new_privs + compile +
 * apply the render-server.json syscall allowlist, then execv).  Invoked by
 * launcher-rs as: sandbox-rs <rs-exec-path> --socket-fd=<n>.
 *
 * The allowlist below is the EXACT render-server.json (106 syscalls), default
 * action TRAP (mismatch_action: "trap") — a disallowed syscall raises SIGSYS,
 * which would manifest in the guest as a venus stall (the chromium exit_code=6
 * symptom) if the RS needs anything outside it.
 *
 * Landlock is intentionally NOT applied here (best-effort defense-in-depth);
 * this isolates the seccomp trap specifically.
 */
#include <errno.h>
#include <linux/filter.h>
#include <linux/seccomp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/prctl.h>
#include <sys/syscall.h>
#include <unistd.h>

#include <seccomp.h>

static void die(const char *what, int rc) {
    fprintf(stderr, "sandbox-rs: %s failed rc=%d errno=%d (%s)\n", what, rc, errno,
            strerror(errno));
    _exit(2);
}

int main(int argc, char **argv) {
    if (argc < 3) {
        fprintf(stderr, "usage: %s <rs-exec-path> --socket-fd=<n>\n", argv[0]);
        return 2;
    }
    const char *rs_path = argv[1];
    const char *socket_opt = argv[2];

    static const char *const allow[] = {
        "accept",        "access",     "arch_prctl",       "bind",
        "brk",           "capget",     "capset",           "clock_gettime",
        "clock_nanosleep", "clone",    "clone3",           "close",
        "connect",       "dup",        "dup2",             "dup3",
        "epoll_create1", "epoll_ctl",  "epoll_pwait",      "epoll_wait",
        "eventfd2",      "execve",     "exit",             "exit_group",
        "faccessat2",    "fchmodat",   "fchownat",         "fcntl",
        "fdatasync",     "flock",      "fstat",            "fstatfs",
        "fsync",         "ftruncate",  "futex",            "getcpu",
        "getcwd",        "getdents64", "getegid",          "geteuid",
        "getgid",        "getpid",     "getrandom",        "getrusage",
        "getsockopt",    "gettid",     "gettimeofday",     "getuid",
        "ioctl",         "kill",       "lseek",            "madvise",
        "membarrier",    "memfd_create", "mkdir",          "mkdirat",
        "mmap",          "mprotect",   "mremap",           "munmap",
        "nanosleep",     "newfstatat", "openat",           "pipe2",
        "poll",          "ppoll",      "prctl",            "pread64",
        "preadv",        "prlimit64",  "pwrite64",         "pwritev",
        "read",          "readlink",   "readlinkat",       "readv",
        "recvfrom",      "recvmsg",    "renameat2",        "rseq",
        "rt_sigaction",  "rt_sigprocmask", "rt_sigreturn", "sched_getaffinity",
        "sched_setaffinity", "sched_setscheduler", "sched_yield", "sendmsg",
        "sendto",        "setpgid",    "setpriority",      "set_robust_list",
        "set_tid_address", "setsockopt", "signalfd4",      "socket",
        "socketpair",    "statx",      "tgkill",           "uname",
        "unlink",        "unlinkat",   "utimensat",        "waitid",
        "write",         "writev",
    };
    const size_t n = sizeof(allow) / sizeof(allow[0]);

    scmp_filter_ctx ctx = seccomp_init(SCMP_ACT_TRAP);
    if (!ctx) die("seccomp_init", 0);
    for (size_t i = 0; i < n; i++) {
        int nr = seccomp_syscall_resolve_name_arch(SCMP_ARCH_NATIVE, allow[i]);
        if (nr < 0) {
            fprintf(stderr, "sandbox-rs: unknown syscall '%s'\n", allow[i]);
            seccomp_release(ctx);
            return 2;
        }
        if (seccomp_rule_add(ctx, SCMP_ACT_ALLOW, (unsigned int)nr, 0) != 0) {
            fprintf(stderr, "sandbox-rs: rule_add failed for %s\n", allow[i]);
            seccomp_release(ctx);
            return 2;
        }
    }
    if (seccomp_load(ctx) != 0) {
        perror("sandbox-rs: seccomp_load");
        seccomp_release(ctx);
        return 2;
    }
    seccomp_release(ctx);
    fprintf(stderr, "sandbox-rs: seccomp filter installed (TRAP default, %zu allowed)\n", n);

    char *argv_child[] = { (char *)"virgl_render_server", (char *)socket_opt, NULL };
    execv(rs_path, argv_child);
    fprintf(stderr, "sandbox-rs: execv(%s) failed errno=%d (%s)\n", rs_path, errno,
            strerror(errno));
    return 127;
}