# loftd vendored libkrun

- Upstream: <https://github.com/containers/libkrun>
- Imported tag: `v1.18.1`
- Imported commit: `b7e43f044d74b77abcfa51175055f72d7cfd6de0`
- Import method: squashed git subtree at `vendor/libkrun`

## Local patch

This vendored copy carries an opt-in profiling patch for loftd launch diagnosis:

- C ABI: `krun_set_profile_path(ctx_id, const char *path)`
- File format: TSV records, `<label>\t<duration_nanos>`
- Behavior: best-effort and disabled unless a caller supplies a profile path
- Purpose: attribute time spent inside `krun_start_enter` / VMM construction before
  libkrun hands control to the guest event loop

To refresh the subtree, prefer the same shape from the repository root:

```bash
git subtree pull --prefix vendor/libkrun https://github.com/containers/libkrun.git v1.18.1 --squash
```

After updating, reapply or upstream the local profiling patch and rebuild `.#libkrun`.
