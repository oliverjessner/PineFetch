# Bundled Whisper runtime

PineFetch 2.7.0 packages a complete CPython installation, standard library and
Whisper dependencies. Preparing or running this runtime does not require an
installed Python or Homebrew Python. The supported release is macOS Apple
silicon. Local transcription requires macOS 14 or later because the pinned
PyAV, NumPy and ONNX Runtime wheels require it. The application itself continues
to support macOS 11; older systems reject a transcription job before downloading
its media and show the version requirement.

## Preparing the runtime

```sh
npm run prepare:whisper-runtime
```

The build uses Node.js and the platform's archive tool. macOS packaging also
requires Apple's Command Line Tools (`install_name_tool`) and `codesign`, as
does the existing native build. No host Python is searched or copied.

- `scripts/whisper-python.json` pins CPython 3.12.15 from the official
  [python-build-standalone release 20261021](https://github.com/astral-sh/python-build-standalone/releases/tag/20261021),
  with a SHA-256 for each archive. It includes the interpreter and standard
  library, rather than a virtual environment referring to another installation.
- `scripts/whisper-runtime.version` and `scripts/whisper-av.version` retain
  faster-whisper 1.2.1 and PyAV 17.1.0. `scripts/whisper-requirements.txt` pins
  their complete dependency set and accepted wheel hashes. pip runs isolated,
  accepts wheels only and requires every downloaded package to match a hash.
- A private staging directory and minimal process environment exclude user
  Python paths, pip configuration and loader overrides. On macOS, unused
  absolute wheel build runpaths are removed, absolute library identities are
  normalized, and changed binaries are signed ad hoc. Actual load dependencies
  are never rewritten to hide a missing library.
- The native audit checks the interpreter and every bundled Mach-O executable,
  extension and library. It requires the correct architecture and deployment
  target, resolves transitive loader/runpath dependencies inside the runtime,
  and permits external dependencies only in macOS system library locations.
  Absolute, broken or escaping symlinks and virtual-environment configuration
  are rejected on every platform.
- A real PCM decode checks Python's relocated prefix/search paths and the
  pinned Whisper/PyAV/Python versions before and after publication. Failed
  installation or validation preserves the previous runtime. An interrupted
  directory swap is recovered on the next preparation; concurrent preparation
  is refused using a build lock.
- A cache is reused only when its preparation inputs match and native/path/
  decoder checks still pass. The model cache and downloaded models are not
  packaged. Models retain the existing first-use download behavior.

The Tauri packaging hook runs preparation even when `tauri build` is invoked
directly. `build:check` remains a compilation check and does not package these
resources. Cargo refreshes its own generated Whisper resource copy while holding its build
lock, preventing legacy venv files or destination symlinks from surviving an
overlay copy. The preparer does not remove another build's target resources.
The shell wrapper is retained for existing build callers; Windows
preparation invokes the same Node implementation and resolves its root-level
`python.exe`. Linux GNU arm64/x64 (glibc 2.28 or later) and Windows x64 have pinned artifacts, but were
not executed or release-qualified in this macOS change. macOS Intel is refused:
the pinned ONNX Runtime release has no Intel macOS wheel. Adding a target
requires verifying its complete wheel set and native runtime.

The original Python and wheel licenses remain in the runtime. Additional
upstream Python distribution notices and artifact provenance are tracked in
[licenses/python-standalone/](licenses/python-standalone/README.md) and copied
into the packaged runtime's `licenses/python-standalone/` directory.

## Real relocated-runtime check

After preparing a runtime, run:

```sh
npm run test:whisper-runtime -- --keep
```

This explicit macOS acceptance check copies the runtime to a temporary path
containing spaces, audits its native dependencies, decodes PCM audio and
transcribes synthetic speech using the real tiny model on CPU. Without
`--model`, it first downloads that model into the isolated temporary directory.
The decode/inference phase runs offline under `sandbox-exec`, with access to
Homebrew, user Python locations, the original repository and the real user home
denied. Missing sandbox tooling fails the check instead of skipping it.

A pre-existing local CTranslate2 model directory can avoid the model download:

```sh
npm run test:whisper-runtime -- --model /absolute/path/to/tiny-model --keep
```

After creating an application bundle, validate its actual resource copy too:

```sh
npm run build -- --bundles app
npm run test:whisper-runtime -- \
  --runtime src-tauri/target/release/bundle/macos/PineFetch.app/Contents/Resources/resources/whisper-runtime \
  --keep
```

The checker leaves evidence when `--keep` is supplied or a check fails. These
checks prove relocation, dependency closure and operation with host Python paths
blocked on the tested host. They do not replace testing installation, signing/
notarization or execution on a separate clean Mac and older supported macOS.
The deterministic `test/whisper-build.test.mjs` and
`test/whisper-portability.test.mjs` fixtures additionally cover installation
failure, rollback, archive integrity, native graphs and malformed binaries;
their synthetic interpreter is not real inference evidence.

When updating Python or any native wheel, refresh the archive/checksum manifest,
requirements hashes and matching upstream notices together. Repeat the moved
bundle test and verify the deployment target before changing compatibility
claims. See the upstream [distribution format](https://gregoryszorc.com/docs/python-build-standalone/main/distributions.html).
