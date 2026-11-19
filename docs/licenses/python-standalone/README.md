# Bundled Python notices

PineFetch bundles CPython 3.12.15 from the official
[python-build-standalone release 20261021](https://github.com/astral-sh/python-build-standalone/releases/tag/20261021).
The matching upstream source commit is
[`0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e`](https://github.com/astral-sh/python-build-standalone/tree/0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e).

The `install_only` archives omit the full archive's `python/licenses/` directory.
This directory restores that upstream notice collection for the runtime bundle.
The twenty component notice files are copied unchanged from the verified macOS
arm64 full archive and match the files at the source commit above. The full
archives for macOS Intel, Linux arm64/x86_64 and Windows x86_64 contain the same
collection; Windows uses CRLF line endings. The collection includes notices for
optional or platform-specific components, so a notice's presence does not mean
that component is present on every target.

`LICENSE.python-build-standalone.txt` is the upstream build project's MPL-2.0
license. CPython and the bundled libraries retain their own licenses; that MPL
file does not replace the component notices. `provenance.json` records the
inspected full archive URLs, SHA-256 digests, target metadata, referenced notice
files and hashes of the copied text files.

The runtime preparation copies this entire directory into
`licenses/python-standalone/` inside the runtime. It also preserves the original
CPython `LICENSE.txt`, pip notices and the licenses shipped in installed wheels.
These additional notices do not replace those original files. A Python release
or distribution update must refresh the pinned artifacts and matching notice
collection together.

Upstream source and build information:

- [Component source archives and checksums](https://github.com/astral-sh/python-build-standalone/blob/0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e/pythonbuild/downloads.json)
- [Build instructions](https://github.com/astral-sh/python-build-standalone/blob/0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e/docs/building.rst)
- [Distribution licensing notes](https://github.com/astral-sh/python-build-standalone/blob/0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e/docs/running.rst#licensing)
- [Archive layout and metadata](https://github.com/astral-sh/python-build-standalone/blob/0c5e581b54a4736cdbbe68bdd6d7e571a1dca11e/docs/distributions.rst)
