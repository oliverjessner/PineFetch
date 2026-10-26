"""Verify bundled dependencies by decoding PCM audio, without downloading a model."""
import io
import sys
import wave
from pathlib import Path

import av
import faster_whisper
from faster_whisper.audio import decode_audio


def main():
    executable = Path(sys.executable).resolve()
    runtime = executable.parent.parent if executable.parent.name == "bin" else executable.parent
    if Path(sys.prefix).resolve() != runtime or Path(sys.base_prefix).resolve() != runtime:
        raise RuntimeError("Bundled Python prefix escapes the standalone runtime")
    for entry in sys.path:
        if not Path(entry).resolve().is_relative_to(runtime):
            raise RuntimeError(f"Python search path escapes the standalone runtime: {entry}")
    for module in (av, faster_whisper, wave):
        if not Path(module.__file__).resolve().is_relative_to(runtime):
            raise RuntimeError(f"Python module escapes the standalone runtime: {module.__name__}")
    expected_whisper, expected_av, expected_python = sys.argv[1:]
    actual = (faster_whisper.__version__, av.__version__, sys.version.split()[0])
    expected = (expected_whisper, expected_av, expected_python)
    if actual != expected:
        raise RuntimeError(f"Whisper runtime versions {actual} do not match {expected}")

    # Imports alone missed faster-whisper 1.2.1's incompatible av.open call in PyAV 19.
    audio = io.BytesIO()
    with wave.open(audio, "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(16000)
        output.writeframes(b"\x00\x00" * 1600)
    audio.seek(0)
    samples = decode_audio(audio, sampling_rate=16000)
    if len(samples) != 1600 or not (samples == 0).all():
        raise RuntimeError("Bundled Whisper runtime could not decode the PCM audio fixture")
    print(f"Whisper audio decode passed: faster-whisper {actual[0]}, PyAV {actual[1]}, Python {actual[2]}")


if __name__ == "__main__":
    main()
