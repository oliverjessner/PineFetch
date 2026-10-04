# GLib security backport

`glib/` contains the published `glib 0.18.5` crate, retaining its MIT license
and copyright notices. The crates.io archive SHA-256 is
`233daaf6e83ae6a12a52055f568f9d7cf4671dabb78ff9560ab6da230ce00ee5`, and its
upstream source commit is `42b9caf98e03ded086362d9653ca58fe94dc8658` in
[gtk-rs-core](https://github.com/gtk-rs/gtk-rs-core/tree/42b9caf98e03ded086362d9653ca58fe94dc8658/glib).

The only library source change is the two-line backport of
[gtk-rs-core#1343](https://github.com/gtk-rs/gtk-rs-core/pull/1343), commit
`b5a4071e439bef2b5eea76c3aa25e5ae84839e34`, fixing
[RUSTSEC-2024-0429 / GHSA-wrw7-89jp-8q8g](https://rustsec.org/advisories/RUSTSEC-2024-0429.html).
`VariantStrIter::impl_get` now passes a mutable out-pointer to
`g_variant_get_child`, so compiler optimizations cannot discard the pointer
write and cause a null dereference.

Tauri 2's GTK3 dependencies require `glib 0.18`, which is incompatible with
the advisory's patched releases (`glib >= 0.20`). The `[patch.crates-io]`
entry in `../Cargo.toml` makes every transitive dependency use this local
backport. The version remains `0.18.5` to preserve dependency compatibility;
version-only scanners may still report the advisory despite the source fix.
Do not remove the patch until Tauri's entire dependency graph uses a patched
upstream GLib release. Check with:

```sh
cargo tree --manifest-path src-tauri/Cargo.toml --locked --target all -i glib
```

Run the upstream iterator regression tests with release optimizations:

```sh
cargo test --manifest-path src-tauri/vendor/glib/Cargo.toml --release --lib variant_iter::tests
```

This test requires native GLib development libraries (`brew install glib` on
macOS or `libglib2.0-dev` on Debian/Ubuntu). Cargo cache metadata is omitted
from the vendored copy. Standalone test lockfiles and build output are ignored.
